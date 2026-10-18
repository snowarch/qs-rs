// The one place that needs QtQuick's private headers, so window.h (included across crates)
// stays on the public API.
#include <QtQuick/QQuickWindow>
#include <QtQuick/private/qquickwindow_p.h>

namespace qsrs {
bool setWindowUpdatesEnabled(QQuickWindow* window, bool enabled) {
	auto* d = QQuickWindowPrivate::get(window);
	if (d->updatesEnabled == enabled) return false;
	d->updatesEnabled = enabled;
	return true;
}
} // namespace qsrs
