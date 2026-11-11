// The compositor's leave to raise a window, asked for the click that activated a tray item (upstream
// service/tray 3c5323b). Without its token an app answering Activate may not take focus: the compositor
// sees a window raising itself.
#pragma once

#include <memory>

#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtCore/QString>
#include <QtCore/QTimer>
#include <QtGui/QGuiApplication>
#include <QtWaylandClient/private/qwaylanddisplay_p.h>
#include <QtWaylandClient/private/qwaylandintegration_p.h>
#include <QtWaylandClient/private/qwaylandwindow_p.h>

namespace qsrs_tray {

// False when no token can be asked for (no Wayland, no click on a surface of ours). Otherwise the core's
// activateWithToken(key, token) runs once: with the token of the window last clicked (its shell surface asks:
// LayerShellQt for panels, Qt for xdg windows and popups), or with none after 250 ms when the surface or the
// compositor never answers, so a click is never lost.
template <typename T>
inline bool request_activation_token(const T& core, const QString& key) {
	if (QGuiApplication::platformName() != QLatin1String("wayland")) return false;
	auto* integration = QtWaylandClient::QWaylandIntegration::instance();
	auto* display = integration ? integration->display() : nullptr;
	auto* window = display ? display->lastInputWindow() : nullptr;
	if (!window || !window->surface() || !display->lastInputDevice()) return false;

	auto* receiver = const_cast<QObject*>(static_cast<const QObject*>(&core));
	auto done = std::make_shared<bool>(false);
	auto deliver = [receiver = QPointer<QObject>(receiver), key, done](const QString& token) {
		if (*done || !receiver) return;
		*done = true;
		QMetaObject::invokeMethod(receiver, "activateWithToken", Q_ARG(QString, key), Q_ARG(QString, token));
	};
	QObject::connect(window, &QtWaylandClient::QWaylandWindow::xdgActivationTokenCreated, receiver, deliver, Qt::SingleShotConnection);
	QTimer::singleShot(250, receiver, [deliver] { deliver(QString()); });
	window->requestXdgActivationToken(display->lastInputSerial());
	return true;
}

} // namespace qsrs_tray
