// The one place that needs Qt's private headers, so window.h and frameclock.h (included
// across crates) stay on the public API.
#include <QtGui/private/qwindow_p.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/private/qquickwindow_p.h>
#include "quickshell/include/frameclock.h"

namespace qsrs {
bool setWindowUpdatesEnabled(QQuickWindow* window, bool enabled) {
	auto* d = QQuickWindowPrivate::get(window);
	if (d->updatesEnabled == enabled) return false;
	d->updatesEnabled = enabled;
	return true;
}

bool frameclock::update_pending(QWindow* window) { return QWindowPrivate::get(window)->updateRequestPending; }
} // namespace qsrs
