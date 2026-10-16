//! D-Bus pixmaps (notification `image-data` hints, StatusNotifierItem icons) written as BMP
//! files that QML loads by path. Qt reads 32-bit BMP v4 with an alpha mask natively, so no
//! encoder is needed.

use std::path::{Path, PathBuf};
use zbus::zvariant::{Structure, Value};

pub struct Pixmap {
    pub width: usize,
    pub height: usize,
    pub rowstride: usize,
    pub channels: usize,
    pub data: Vec<u8>,
}

impl Pixmap {
    /// Parses the hint value; `None` (with a warning) when its layout is not the 8-bit
    /// RGB/RGBA one the spec allows.
    pub fn from_value(value: &Value) -> Option<Self> {
        let value = match value {
            Value::Value(v) => &**v,
            v => v,
        };
        let s: Structure = value.try_clone().ok()?.try_into().ok()?;
        let f = s.fields();
        if f.len() != 7 {
            return None;
        }
        let int = |v: &Value| i32::try_from(v).ok();
        let (w, h, stride) = (int(&f[0])?, int(&f[1])?, int(&f[2])?);
        let alpha = bool::try_from(&f[3]).ok()?;
        let (bits, channels) = (int(&f[4])?, int(&f[5])?);
        let data: Vec<u8> = f[6].try_clone().ok()?.try_into().ok()?;
        let want = if alpha { 4 } else { 3 };
        if bits != 8 || channels != want || w <= 0 || h <= 0 || stride < w * channels {
            eprintln!("Notifications: unsupported pixmap {w}x{h} stride {stride} bits {bits} channels {channels}");
            return None;
        }
        let (w, h, stride, channels) = (w as usize, h as usize, stride as usize, channels as usize);
        if data.len() < stride * (h - 1) + w * channels {
            eprintln!("Notifications: pixmap data too short");
            return None;
        }
        Some(Self { width: w, height: h, rowstride: stride, channels, data })
    }

    /// A StatusNotifierItem pixmap: ARGB32 in network byte order, rows packed.
    pub fn from_argb32(width: i32, height: i32, data: &[u8]) -> Option<Self> {
        let (w, h) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        if w == 0 || h == 0 || data.len() < w * h * 4 {
            return None;
        }
        let rgba = data[..w * h * 4].chunks_exact(4).flat_map(|p| [p[1], p[2], p[3], p[0]]).collect();
        Some(Self { width: w, height: h, rowstride: w * 4, channels: 4, data: rgba })
    }

    /// The pixmap as a BMP v4 file (32-bit BGRA, bottom-up rows).
    pub fn to_bmp(&self) -> Vec<u8> {
        let (w, h) = (self.width, self.height);
        let pixels = w * h * 4;
        let mut out = Vec::with_capacity(14 + 108 + pixels);
        let u16le = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
        let u32le = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(b"BM");
        u32le(&mut out, (14 + 108 + pixels) as u32);
        u32le(&mut out, 0);
        u32le(&mut out, 14 + 108);
        u32le(&mut out, 108); // BITMAPV4HEADER
        u32le(&mut out, w as u32);
        u32le(&mut out, h as u32);
        u16le(&mut out, 1);
        u16le(&mut out, 32);
        u32le(&mut out, 3); // BI_BITFIELDS
        u32le(&mut out, pixels as u32);
        u32le(&mut out, 2835);
        u32le(&mut out, 2835);
        u32le(&mut out, 0);
        u32le(&mut out, 0);
        for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
            u32le(&mut out, mask);
        }
        u32le(&mut out, 0x7352_4742); // LCS_sRGB
        out.extend_from_slice(&[0; 48]); // endpoints and gamma, unused for sRGB
        for y in (0..h).rev() {
            let row = &self.data[y * self.rowstride..];
            for x in 0..w {
                let p = &row[x * self.channels..];
                let a = if self.channels == 4 { p[3] } else { 255 };
                out.extend_from_slice(&[p[2], p[1], p[0], a]);
            }
        }
        out
    }
}

/// A per-process directory under `$XDG_RUNTIME_DIR/qs-rs` for pixmap files. Directories left by
/// processes that are gone are removed on first use.
pub fn image_dir(kind: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("qs-rs");
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(pid) = name.strip_prefix(kind).and_then(|r| r.strip_prefix('-')) {
                if !Path::new("/proc").join(pid).exists() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
    base.join(format!("{kind}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_with_padded_rows() {
        // 1x2 RGB, rowstride 4: red over green.
        let v = Value::from(Structure::from((1i32, 2i32, 4i32, false, 8i32, 3i32, vec![255u8, 0, 0, 0, 0, 255, 0])));
        let p = Pixmap::from_value(&v).unwrap();
        let bmp = p.to_bmp();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 14 + 108 + 8);
        // bottom-up: green row first, then red, as BGRA
        assert_eq!(&bmp[122..], &[0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn argb_network_order() {
        let p = Pixmap::from_argb32(1, 1, &[128, 1, 2, 3]).unwrap();
        assert_eq!(p.data, vec![1, 2, 3, 128]);
    }

    #[test]
    fn rejects_bad_layout() {
        let v = Value::from(Structure::from((1i32, 1i32, 4i32, true, 8i32, 3i32, vec![0u8; 4])));
        assert!(Pixmap::from_value(&v).is_none());
    }
}
