#pragma once
// The frame clock qs-rs's animation driver (qs-rs/include/frames.h) keeps for native animation
// types (SpringValue): when the current animation step is meant to be on screen, and the
// refresh period it steps at. GUI thread only; zero while the driver is not installed
// (QSRS_QT_ANIMATION_TIMER), and readers fall back to their own clock.
#include <QtCore/QString>
#include <QtCore/QtGlobal>
#include <ctime>

class QWindow;

namespace qsrs::frameclock {
inline qint64 now_ns() {
	timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return qint64(ts.tv_sec) * 1000000000 + ts.tv_nsec;
}

/// CLOCK_MONOTONIC ns the last driver step is presented at (a predicted vblank, or the time
/// of a fallback timer tick), and the refresh period the driver steps at.
inline qint64 step_ns = 0;
inline qint64 period_ns = 0;

/// Whether the window has asked for a frame it has not got yet (QWindow::requestUpdate),
/// read from QWindowPrivate (src/cpp/windowprivate.cpp).
bool update_pending(QWindow* window);

/// Whether a drawable QQuickWindow holds dirtied items or items waiting for polish: a frame it
/// needs, asked for or not (src/cpp/windowprivate.cpp).
bool scene_waits(QWindow* window);

/// What dirtied a QQuickWindow's coming frame: its dirty items as `Type@file.qml:line`, most
/// repeated first, `|`-joined, at most `max` and a `+N` for the rest (src/cpp/windowprivate.cpp).
QString dirty_items(QWindow* window, int max);
} // namespace qsrs::frameclock
