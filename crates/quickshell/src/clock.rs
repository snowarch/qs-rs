//! Native Rust `SystemClock`: wall clock aligned to whole second, minute, or hour boundaries.
//! Replaces the QML/JS `SystemClock.qml` wrapper: zero per-tick JS Date allocations, zero extra
//! QTimer child objects, and `hoursChanged`/`minutesChanged`/`secondsChanged` emit only when their
//! respective field actually changes.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qdatetime.h");
        type QDateTime = cxx_qt_lib::QDateTime;

        include!("quickshell/include/qsrs.h");
        #[namespace = "qsrs"]
        fn datetime_from_ms(ms: i64) -> QDateTime;
        #[namespace = "qsrs"]
        unsafe fn schedule_clock(target: *mut QObject, delay_ms: i32, generation: u64);
    }

    #[qenum(SystemClock)]
    enum Precision {
        None,
        Hours,
        Minutes,
        Seconds,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, enabled, READ, WRITE = set_enabled, NOTIFY)]
        #[qproperty(i32, precision, READ, WRITE = set_precision, NOTIFY)]
        #[qproperty(QDateTime, date, READ, NOTIFY)]
        #[qproperty(i32, hours, READ, NOTIFY)]
        #[qproperty(i32, minutes, READ, NOTIFY)]
        #[qproperty(i32, seconds, READ, NOTIFY)]
        type SystemClock = super::SystemClockRust;

        fn set_enabled(self: Pin<&mut SystemClock>, enabled: bool);
        fn set_precision(self: Pin<&mut SystemClock>, precision: i32);

        #[qinvokable]
        #[cxx_name = "_qsTick"]
        fn on_tick(self: Pin<&mut SystemClock>, generation: u64);
    }

    impl cxx_qt::Initialize for SystemClock {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::QDateTime;

#[cfg(test)]
const HOURS: i32 = 1;
const MINUTES: i32 = 2;
const SECONDS: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WallSnap {
    unix_ms: i64,
    hours: i32,
    minutes: i32,
    seconds: i32,
    next_target_ms: i64,
}

fn now_unix_ms() -> i64 {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    ts.tv_sec as i64 * 1000 + ts.tv_nsec as i64 / 1_000_000
}

fn local_hms(unix_secs: i64) -> (i32, i32, i32) {
    let t = unix_secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return (0, 0, 0);
    }
    (tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn snap(base_ms: i64, precision: i32) -> WallSnap {
    let secs = base_ms.div_euclid(1000);
    let (h, m, s) = local_hms(secs);
    let (trunc_secs, hours, minutes, seconds, step_secs) = if precision >= SECONDS {
        (secs, h, m, s, 1)
    } else if precision >= MINUTES {
        (secs - s as i64, h, m, 0, 60)
    } else {
        (secs - (m as i64 * 60 + s as i64), h, 0, 0, 3600)
    };
    WallSnap {
        unix_ms: trunc_secs * 1000,
        hours,
        minutes,
        seconds,
        next_target_ms: (trunc_secs + step_secs) * 1000,
    }
}

pub struct SystemClockRust {
    enabled: bool,
    precision: i32,
    date: QDateTime,
    hours: i32,
    minutes: i32,
    seconds: i32,
    unix_ms: i64,
    target_ms: Option<i64>,
    generation: u64,
}

impl Default for SystemClockRust {
    fn default() -> Self {
        let s = snap(now_unix_ms(), SECONDS);
        Self {
            enabled: true,
            precision: SECONDS,
            date: qobject::datetime_from_ms(s.unix_ms),
            hours: s.hours,
            minutes: s.minutes,
            seconds: s.seconds,
            unix_ms: s.unix_ms,
            target_ms: None,
            generation: 0,
        }
    }
}

impl cxx_qt::Initialize for qobject::SystemClock {
    fn initialize(self: Pin<&mut Self>) {
        self.schedule();
    }
}

impl qobject::SystemClock {
    fn set_enabled(mut self: Pin<&mut Self>, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.as_mut().rust_mut().enabled = enabled;
        self.as_mut().enabled_changed();
        if enabled {
            self.as_mut().refresh(None);
            self.schedule();
        } else {
            let mut rust = self.as_mut().rust_mut();
            rust.generation = rust.generation.wrapping_add(1);
            rust.target_ms = None;
        }
    }

    fn set_precision(mut self: Pin<&mut Self>, precision: i32) {
        if self.precision == precision {
            return;
        }
        self.as_mut().rust_mut().precision = precision;
        self.as_mut().precision_changed();
        if self.enabled {
            self.as_mut().refresh(None);
            self.schedule();
        } else {
            let s = snap(self.unix_ms, precision);
            self.apply_snap(s);
        }
    }

    fn on_tick(mut self: Pin<&mut Self>, generation: u64) {
        if !self.enabled || generation != self.generation {
            return;
        }
        let target = self.target_ms;
        self.as_mut().refresh(target);
        self.schedule();
    }

    fn refresh(self: Pin<&mut Self>, target: Option<i64>) {
        let now = now_unix_ms();
        let base = match target {
            Some(t) if (t - now).abs() < 500 => t,
            _ => now,
        };
        let s = snap(base, self.precision);
        self.apply_snap(s);
    }

    fn apply_snap(mut self: Pin<&mut Self>, s: WallSnap) {
        let mut rust = self.as_mut().rust_mut();
        let date_ch = rust.unix_ms != s.unix_ms;
        let h_ch = rust.hours != s.hours;
        let m_ch = rust.minutes != s.minutes;
        let s_ch = rust.seconds != s.seconds;
        if date_ch {
            rust.unix_ms = s.unix_ms;
            rust.date = qobject::datetime_from_ms(s.unix_ms);
        }
        rust.hours = s.hours;
        rust.minutes = s.minutes;
        rust.seconds = s.seconds;
        if date_ch {
            self.as_mut().date_changed();
        }
        if h_ch {
            self.as_mut().hours_changed();
        }
        if m_ch {
            self.as_mut().minutes_changed();
        }
        if s_ch {
            self.as_mut().seconds_changed();
        }
    }

    fn schedule(mut self: Pin<&mut Self>) {
        if !self.enabled {
            return;
        }
        let now = now_unix_ms();
        let next = snap(now, self.precision).next_target_ms;
        let delay = (next - now).clamp(1, i32::MAX as i64) as i32;
        let gen_id = {
            let mut rust = self.as_mut().rust_mut();
            rust.generation = rust.generation.wrapping_add(1);
            rust.target_ms = Some(next);
            rust.generation
        };
        // SAFETY: `self` is a live QObject on the GUI thread; QTimer::singleShot ties the callback
        // lifetime to `ptr`.
        let ptr = unsafe { self.as_mut().get_unchecked_mut() } as *mut Self as *mut cxx_qt::QObject;
        unsafe { qobject::schedule_clock(ptr, delay, gen_id) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_truncates_per_precision_and_advances_boundary() {
        let base_ms = now_unix_ms();
        let sec = snap(base_ms, SECONDS);
        assert_eq!(sec.unix_ms % 1000, 0);
        assert_eq!(sec.next_target_ms - sec.unix_ms, 1000);

        let min = snap(base_ms, MINUTES);
        assert_eq!(min.seconds, 0);
        assert_eq!(min.next_target_ms - min.unix_ms, 60_000);

        let hr = snap(base_ms, HOURS);
        assert_eq!(hr.minutes, 0);
        assert_eq!(hr.seconds, 0);
        assert_eq!(hr.next_target_ms - hr.unix_ms, 3_600_000);
    }
}
