#pragma once
// The wl_display Qt's wayland platform plugin is connected to, and the mapping between Qt's
// wl_output proxies and QScreens (public native interfaces).
#include <cstddef>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtGui/qscreen_platform.h>

namespace qswl {
inline std::size_t qt_wl_display() {
	auto* app = qGuiApp ? qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>() : nullptr;
	return app ? reinterpret_cast<std::size_t>(app->display()) : 0;
}

inline std::size_t screen_output(QScreen* screen) {
	auto* ws = screen ? screen->nativeInterface<QNativeInterface::QWaylandScreen>() : nullptr;
	return ws ? reinterpret_cast<std::size_t>(ws->output()) : 0;
}

/// Name of the QScreen backed by this wl_output, or an empty string.
inline QString screen_name_for_output(std::size_t output) {
	for (auto* screen: QGuiApplication::screens()) {
		if (output != 0 && screen_output(screen) == output) return screen->name();
	}
	return {};
}

/// Qt's wl_output for the screen with this name, or 0.
inline std::size_t output_for_screen_name(const QString& name) {
	for (auto* screen: QGuiApplication::screens()) {
		if (screen->name() == name) return screen_output(screen);
	}
	return 0;
}
} // namespace qswl
