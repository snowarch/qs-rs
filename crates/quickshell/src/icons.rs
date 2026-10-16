//! Icon URLs served by the `image://icon` provider (see include/qsrs.h).

use cxx_qt_lib::QString;

pub fn request_string(icon: &str, fallback: Option<&str>) -> QString {
    let mut s = format!("image://icon/{icon}");
    if let Some(f) = fallback.filter(|f| !f.is_empty()) {
        s.push_str("?fallback=");
        s.push_str(f);
    }
    QString::from(&s)
}

pub fn lookup(icon: &str) -> Option<()> {
    crate::util::has_theme_icon(&QString::from(icon)).then_some(())
}
