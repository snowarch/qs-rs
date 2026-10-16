//! Stream parsers assignable to `Process.stdout` / `Process.stderr`.
//! The byte buffer belongs to the stream owner; parsers only decide what to emit.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, split_marker, cxx_name = "splitMarker")]
        type SplitParser = super::SplitParserRust;

        #[qsignal]
        fn read(self: Pin<&mut SplitParser>, data: QString);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, wait_for_end, cxx_name = "waitForEnd")]
        #[qproperty(QString, text, READ = text, NOTIFY = data_changed)]
        #[qproperty(QByteArray, data, READ = data, NOTIFY = data_changed)]
        type StdioCollector = super::StdioCollectorRust;

        fn text(self: &StdioCollector) -> QString;
        fn data(self: &StdioCollector) -> QByteArray;

        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(self: Pin<&mut StdioCollector>);
        #[qsignal]
        #[cxx_name = "streamFinished"]
        fn stream_finished(self: Pin<&mut StdioCollector>);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};

pub struct SplitParserRust {
    split_marker: QString,
}

impl Default for SplitParserRust {
    fn default() -> Self {
        Self { split_marker: QString::from("\n") }
    }
}

pub struct StdioCollectorRust {
    wait_for_end: bool,
    data: Vec<u8>,
}

impl Default for StdioCollectorRust {
    fn default() -> Self {
        Self { wait_for_end: true, data: Vec::new() }
    }
}

impl qobject::StdioCollector {
    pub fn text(&self) -> QString {
        QString::from(&*String::from_utf8_lossy(&self.rust().data))
    }

    pub fn data(&self) -> QByteArray {
        QByteArray::from(self.rust().data.as_slice())
    }
}

/// Splits `buffer + incoming` on `marker`, leaving the unterminated tail in `buffer`.
pub fn split(marker: &[u8], buffer: &mut Vec<u8>, incoming: &[u8]) -> Vec<Vec<u8>> {
    buffer.extend_from_slice(incoming);
    if marker.is_empty() {
        return if buffer.is_empty() { vec![] } else { vec![std::mem::take(buffer)] };
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + marker.len() <= buffer.len() {
        if &buffer[i..i + marker.len()] == marker {
            out.push(buffer[start..i].to_vec());
            i += marker.len();
            start = i;
        } else {
            i += 1;
        }
    }
    buffer.drain(..start);
    out
}

/// A parser target resolved from the `QObject*` stored in a stream property.
pub enum Parser<'a> {
    Split(Pin<&'a mut qobject::SplitParser>),
    Collector(Pin<&'a mut qobject::StdioCollector>),
}

impl Parser<'_> {
    /// # Safety
    /// `ptr` must be null or point to a live QObject.
    pub unsafe fn from_ptr<'a>(ptr: *mut cxx_qt::QObject) -> Option<Parser<'a>> {
        use cxx_qt::casting::Downcast;
        let obj = unsafe { Pin::new_unchecked(ptr.as_mut()?) };
        let obj2 = unsafe { Pin::new_unchecked(&mut *ptr) };
        if let Some(p) = obj.downcast_pin::<qobject::SplitParser>() {
            return Some(Parser::Split(p));
        }
        obj2.downcast_pin::<qobject::StdioCollector>().map(Parser::Collector)
    }

    pub fn feed(self, buffer: &mut Vec<u8>, incoming: &[u8]) {
        match self {
            Parser::Split(mut p) => {
                let marker = p.split_marker().to_string().into_bytes();
                for part in split(&marker, buffer, incoming) {
                    p.as_mut().read(QString::from(&*String::from_utf8_lossy(&part)));
                }
            }
            Parser::Collector(mut p) => {
                buffer.extend_from_slice(incoming);
                if !*p.wait_for_end() {
                    p.as_mut().rust_mut().data = buffer.clone();
                    p.as_mut().data_changed();
                }
            }
        }
    }

    pub fn end(self, buffer: &mut Vec<u8>) {
        match self {
            Parser::Split(mut p) => {
                if !buffer.is_empty() {
                    p.as_mut().read(QString::from(&*String::from_utf8_lossy(buffer)));
                }
            }
            Parser::Collector(mut p) => {
                if *p.wait_for_end() {
                    p.as_mut().rust_mut().data = buffer.clone();
                    p.as_mut().data_changed();
                }
                p.as_mut().stream_finished();
            }
        }
        buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn split_keeps_partial_tail() {
        let mut buf = Vec::new();
        assert_eq!(split(b"\n", &mut buf, b"a\nb"), vec![b"a".to_vec()]);
        assert_eq!(buf, b"b");
        assert_eq!(split(b"\n", &mut buf, b"c\n\nd"), vec![b"bc".to_vec(), b"".to_vec()]);
        assert_eq!(buf, b"d");
    }

    #[test]
    fn split_multibyte_marker_across_chunks() {
        let mut buf = Vec::new();
        assert!(split(b"--", &mut buf, b"x-").is_empty());
        assert_eq!(split(b"--", &mut buf, b"-y"), vec![b"x".to_vec()]);
        assert_eq!(buf, b"y");
    }
}
