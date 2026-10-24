//! The subset of Qt's QDataStream binary format (big endian, Qt 6) that Quickshell's instance
//! lock and IPC messages use, so the upstream `qs` CLI and qs-rs read each other's bytes.

#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    /// QString: byte length (0xffffffff for a null string) then UTF-16BE.
    pub fn string(&mut self, s: &str) -> &mut Self {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.u32((units.len() * 2) as u32);
        for u in units {
            self.0.extend_from_slice(&u.to_be_bytes());
        }
        self
    }
    /// QList/QVector: element count then the elements.
    pub fn list<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) -> &mut Self {
        self.u32(items.len() as u32);
        for i in items {
            f(self, i);
        }
        self
    }
}

/// Reading past the end means the message is not complete yet (like a failed QDataStream
/// transaction): callers wait for more bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct Incomplete;

pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Incomplete> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(Incomplete)?;
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, Incomplete> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool, Incomplete> {
        Ok(self.u8()? != 0)
    }
    pub fn i8(&mut self) -> Result<i8, Incomplete> {
        Ok(self.u8()? as i8)
    }
    pub fn u32(&mut self) -> Result<u32, Incomplete> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, Incomplete> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64, Incomplete> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn string(&mut self) -> Result<String, Incomplete> {
        let len = self.u32()?;
        if len == u32::MAX {
            return Ok(String::new());
        }
        let bytes = self.take(len as usize)?;
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }
    pub fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T, Incomplete>) -> Result<Vec<T>, Incomplete> {
        let n = self.u32()?;
        (0..n).map(|_| f(self)).collect()
    }
}

const UNIX_EPOCH_JULIAN_DAY: i64 = 2_440_588;

/// QDateTime in local time (Qt::LocalTime, what QDateTime::currentDateTime() gives): QDate as
/// julian day, QTime as milliseconds since midnight, then the time spec.
pub fn write_local_datetime(w: &mut Writer, unix_ms: i64) {
    let secs = unix_ms.div_euclid(1000);
    let offset = local_offset(secs);
    let local_ms = unix_ms + offset * 1000;
    let day = local_ms.div_euclid(86_400_000);
    w.i64(UNIX_EPOCH_JULIAN_DAY + day).u32(local_ms.rem_euclid(86_400_000) as u32).i8(0);
}

/// Reads a QDateTime back to unix milliseconds (local, UTC and fixed-offset specs).
pub fn read_datetime(r: &mut Reader) -> Result<i64, Incomplete> {
    let jd = r.i64()?;
    let ms = r.u32()?;
    let spec = r.i8()?;
    let wall = (jd - UNIX_EPOCH_JULIAN_DAY) * 86_400_000 + if ms == u32::MAX { 0 } else { ms as i64 };
    Ok(match spec {
        1 => wall,
        2 => wall - r.i32()? as i64 * 1000,
        _ => wall - local_offset(wall.div_euclid(1000)) * 1000,
    })
}

fn local_offset(unix_secs: i64) -> i64 {
    let t = unix_secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_is_utf16_with_byte_length() {
        let mut w = Writer::default();
        w.string("añ");
        assert_eq!(w.0, [0, 0, 0, 4, 0, b'a', 0, 0xf1]);
        assert_eq!(Reader::new(&w.0).string().unwrap(), "añ");
        assert_eq!(Reader::new(&[0xff, 0xff, 0xff, 0xff]).string().unwrap(), "");
    }

    #[test]
    fn incomplete_input_is_reported() {
        let mut w = Writer::default();
        w.list(&["x".to_string(), "yz".to_string()], |w, s| { w.string(s); });
        for cut in 0..w.0.len() {
            assert_eq!(Reader::new(&w.0[..cut]).list(|r| r.string()), Err(Incomplete));
        }
        assert_eq!(Reader::new(&w.0).list(|r| r.string()).unwrap(), ["x", "yz"]);
    }

    #[test]
    fn datetime_round_trips() {
        let mut w = Writer::default();
        write_local_datetime(&mut w, 1_760_000_000_123);
        assert_eq!(w.0.len(), 13);
        assert_eq!(read_datetime(&mut Reader::new(&w.0)).unwrap(), 1_760_000_000_123);
    }
}
