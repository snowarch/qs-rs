//! `ColorQuantizer`: dominant colors of an image by median cut (2^depth buckets), computed on a
//! worker thread. Same algorithm as Quickshell's colorquantizer.cpp.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        include!("cxx-qt-lib/qrect.h");
        type QRect = cxx_qt_lib::QRect;
        include!("cxx-qt-lib/qlist.h");
        type QList_QColor = cxx_qt_lib::QList<cxx_qt_lib::QColor>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QList_QColor, colors, READ, NOTIFY)]
        #[qproperty(QUrl, source, READ, WRITE = set_source, NOTIFY)]
        #[qproperty(f64, depth, READ, WRITE = set_depth, NOTIFY)]
        #[qproperty(QRect, image_rect, cxx_name = "imageRect", READ, WRITE = set_image_rect, NOTIFY)]
        #[qproperty(f64, rescale_size, cxx_name = "rescaleSize", READ, WRITE = set_rescale_size, NOTIFY)]
        type ColorQuantizer = super::ColorQuantizerRust;

        fn set_source(self: Pin<&mut ColorQuantizer>, source: QUrl);
        fn set_depth(self: Pin<&mut ColorQuantizer>, depth: f64);
        fn set_image_rect(self: Pin<&mut ColorQuantizer>, rect: QRect);
        fn set_rescale_size(self: Pin<&mut ColorQuantizer>, size: f64);
    }

    impl cxx_qt::Threading for ColorQuantizer {}
    impl cxx_qt::Initialize for ColorQuantizer {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QColor, QList, QRect, QString, QUrl};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type DecodeKey = (String, u128, i32, i32, i32, i32, u64);
/// Small decodes in flight or just done, shared: inir's palette and its backdrop ask for the same wallpaper at the
/// same size at once, and a second ask waits for the first instead of decoding it again. Keyed by what changes the
/// pixels (the file's mtime among them); the last few are kept.
static DECODES: Mutex<Vec<(DecodeKey, Arc<OnceLock<Arc<Vec<u32>>>>)>> = Mutex::new(Vec::new());

fn pixels(path: &str, (x, y, w, h): (i32, i32, i32, i32), rescale: f64) -> Arc<Vec<u32>> {
    let load = || Arc::new(crate::util::load_pixels(&QString::from(path), x, y, w, h, rescale));
    // Only shrunk reads are kept: a whole picture would hold tens of MB.
    if !(rescale > 0.0 && rescale <= 256.0) {
        return load();
    }
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let stamp = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let key = (path.to_string(), stamp, x, y, w, h, rescale.to_bits());
    let cell = {
        let mut all = DECODES.lock().unwrap();
        match all.iter().find(|(k, _)| *k == key) {
            Some((_, cell)) => cell.clone(),
            None => {
                let cell = Arc::new(OnceLock::new());
                all.push((key, cell.clone()));
                if all.len() > 4 {
                    all.remove(0);
                }
                cell
            }
        }
    };
    cell.get_or_init(load).clone()
}

#[derive(Default)]
pub struct ColorQuantizerRust {
    colors: QList<QColor>,
    source: QUrl,
    depth: f64,
    image_rect: QRect,
    rescale_size: f64,
    completed: bool,
    run: u64,
    cancel: Option<Arc<AtomicBool>>,
}

/// Median cut over 0xAARRGGBB pixels (fully transparent ones skipped); returns RGB triples.
pub fn quantize(pixels: &[u32], max_depth: f64, cancel: &AtomicBool) -> Vec<[u8; 3]> {
    let mut rgb: Vec<[u8; 3]> = pixels.iter().filter(|p| *p >> 24 != 0).map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut out = Vec::new();
    split(&mut rgb, 0.0, max_depth, cancel, &mut out);
    out
}

fn split(values: &mut [[u8; 3]], depth: f64, max_depth: f64, cancel: &AtomicBool, out: &mut Vec<[u8; 3]>) {
    if cancel.load(Ordering::Acquire) || values.is_empty() {
        return;
    }
    if depth >= max_depth {
        let n = values.len() as f64;
        let sum = values.iter().fold([0u64; 3], |acc, c| [acc[0] + c[0] as u64, acc[1] + c[1] as u64, acc[2] + c[2] as u64]);
        out.push([0, 1, 2].map(|i| (sum[i] as f64 / n).round() as u8));
        return;
    }
    let range = |i: usize| {
        let (lo, hi) = values.iter().fold((255u8, 0u8), |(lo, hi), c| (lo.min(c[i]), hi.max(c[i])));
        hi as i32 - lo as i32
    };
    let (r, g, b) = (range(0), range(1), range(2));
    let biggest = r.max(g).max(b);
    let channel = if biggest == r { 0 } else if biggest == g { 1 } else { 2 };
    values.sort_by_key(|c| c[channel]);
    let mid = values.len() / 2;
    let (left, right) = values.split_at_mut(mid);
    split(left, depth + 1.0, max_depth, cancel, out);
    split(right, depth + 1.0, max_depth, cancel, out);
}

impl cxx_qt::Initialize for qobject::ColorQuantizer {
    fn initialize(self: Pin<&mut Self>) {
        // Properties from QML are applied by the next event loop turn.
        let _ = self.qt_thread().queue(|mut this| {
            this.as_mut().rust_mut().completed = true;
            this.start();
        });
    }
}

impl qobject::ColorQuantizer {
    fn set_source(mut self: Pin<&mut Self>, source: QUrl) {
        if source != self.source {
            self.as_mut().rust_mut().source = source;
            self.as_mut().source_changed();
            self.start();
        }
    }
    fn set_depth(mut self: Pin<&mut Self>, depth: f64) {
        if depth != self.depth {
            self.as_mut().rust_mut().depth = depth;
            self.as_mut().depth_changed();
            self.start();
        }
    }
    fn set_image_rect(mut self: Pin<&mut Self>, rect: QRect) {
        if rect != self.image_rect {
            self.as_mut().rust_mut().image_rect = rect;
            self.as_mut().image_rect_changed();
            self.start();
        }
    }
    fn set_rescale_size(mut self: Pin<&mut Self>, size: f64) {
        if size != self.rescale_size {
            self.as_mut().rust_mut().rescale_size = size;
            self.as_mut().rescale_size_changed();
            self.start();
        }
    }

    fn start(mut self: Pin<&mut Self>) {
        let path = self.source.to_local_file().map(|p| p.to_string()).unwrap_or_default();
        if !self.completed || path.is_empty() {
            return;
        }
        if let Some(c) = self.as_mut().rust_mut().cancel.take() {
            c.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut rust = self.as_mut().rust_mut();
        rust.cancel = Some(cancel.clone());
        rust.run += 1;
        let run = rust.run;
        let r = &self.image_rect;
        let (x, y, w, h) = (r.x(), r.y(), r.width(), r.height());
        let (depth, rescale) = (self.depth, self.rescale_size);
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let pixels = pixels(&path, (x, y, w, h), rescale);
            if pixels.is_empty() {
                crate::qs_log!(warn, "quickshell.colorquantizer", "Failed to load image from {path}");
            }
            let colors = quantize(&pixels, depth, &cancel);
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = qt.queue(move |mut this| {
                if this.run != run {
                    return;
                }
                let mut list = QList::<QColor>::default();
                for c in colors {
                    list.append(QColor::from_rgb(c[0] as i32, c[1] as i32, c[2] as i32));
                }
                this.as_mut().rust_mut().colors = list;
                this.as_mut().colors_changed();
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_clusters_at_depth_one() {
        let px: Vec<u32> = (0..10).map(|_| 0xffff0000).chain((0..10).map(|_| 0xff0000ff)).chain([0x00ffffff]).collect();
        let mut c = quantize(&px, 1.0, &AtomicBool::new(false));
        c.sort();
        assert_eq!(c, vec![[0, 0, 255], [255, 0, 0]]);
    }
}
