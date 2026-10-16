#pragma once
// The wl_display Qt's wayland platform plugin is connected to (public native interface).
#include <cstddef>
#include <QtGui/QGuiApplication>

namespace qswl {
inline std::size_t qt_wl_display() {
	auto* app = qGuiApp ? qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>() : nullptr;
	return app ? reinterpret_cast<std::size_t>(app->display()) : 0;
}
} // namespace qswl
