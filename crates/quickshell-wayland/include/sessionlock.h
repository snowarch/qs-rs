#pragma once
// WlSessionLock over ext-session-lock-v1, ported from upstream src/wayland/session_lock*. A lock
// surface needs its role before QtWayland would give the wl_surface one, so lock windows get
// their own QtWayland shell integration (private QtWaylandClient API, like upstream and
// LayerShellQt). Protocol objects live on Qt's default queue: every callback runs on the Qt
// thread. WlSessionLockSurface.qml provides the window; this file creates one per screen.
#include <QtCore/QMap>
#include <QtCore/QPointer>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlParserStatus>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtWaylandClient/private/qwaylanddisplay_p.h>
#include <QtWaylandClient/private/qwaylandintegration_p.h>
#include <QtWaylandClient/private/qwaylandscreen_p.h>
#include <QtWaylandClient/private/qwaylandshellintegration_p.h>
#include <QtWaylandClient/private/qwaylandshellsurface_p.h>
#include <QtWaylandClient/private/qwaylandsurface_p.h>
#include <QtWaylandClient/private/qwaylandwindow_p.h>

#include "ext-session-lock-v1-client-protocol.h"

class WlSessionLock;

namespace qssl {

inline ext_session_lock_manager_v1* manager() {
	static ext_session_lock_manager_v1* manager = []() -> ext_session_lock_manager_v1* {
		auto* integration = QtWaylandClient::QWaylandIntegration::instance();
		if (integration == nullptr) return nullptr;
		for (const auto& global: integration->display()->globals()) {
			if (global.interface == QLatin1String(ext_session_lock_manager_v1_interface.name)) {
				return static_cast<ext_session_lock_manager_v1*>(
				    wl_registry_bind(global.registry, global.id, &ext_session_lock_manager_v1_interface, 1)
				);
			}
		}
		return nullptr;
	}();
	return manager;
}

/// The one lock this process may hold.
struct ActiveLock {
	ext_session_lock_v1* lock = nullptr;
	QPointer<WlSessionLock> owner;
	bool secure = false;
	// Unlocked before the compositor answered: the lock and its surfaces are kept until `locked`
	// or `finished`, since destroying an unconfirmed lock makes niri refuse the next lock
	// surface on those outputs. Locking again in the meantime takes them back.
	bool unlockPending = false;
	QMap<QScreen*, QPointer<QObject>> lingering;
};

inline ActiveLock& active() {
	static ActiveLock lock;
	return lock;
}

inline wl_output* outputFor(QScreen* screen) {
	auto* ws = screen ? dynamic_cast<QtWaylandClient::QWaylandScreen*>(screen->handle()) : nullptr;
	return ws ? ws->output() : nullptr;
}

class LockSurface: public QtWaylandClient::QWaylandShellSurface {
public:
	explicit LockSurface(QtWaylandClient::QWaylandWindow* window): QWaylandShellSurface(window) {
		this->surface = ext_session_lock_v1_get_lock_surface(
		    active().lock,
		    window->waylandSurface()->object(),
		    outputFor(window->window()->screen())
		);
		ext_session_lock_surface_v1_add_listener(this->surface, &listener, this);
	}
	~LockSurface() override { ext_session_lock_surface_v1_destroy(this->surface); }
	Q_DISABLE_COPY_MOVE(LockSurface);

	[[nodiscard]] bool isExposed() const override { return this->configured; }
	void applyConfigure() override { this->window()->resizeFromApplyConfigure(this->size); }
	// The role comes from get_lock_surface; nothing may be committed before the first configure.
	[[nodiscard]] bool commitSurfaceRole() const override { return false; }

private:
	static void onConfigure(void* data, ext_session_lock_surface_v1* surface, uint32_t serial, uint32_t width, uint32_t height) {
		auto* self = static_cast<LockSurface*>(data);
		if (self->window() == nullptr) return;
		ext_session_lock_surface_v1_ack_configure(surface, serial);
		self->size = QSize(static_cast<int>(width), static_cast<int>(height));
		self->window()->resizeFromApplyConfigure(self->size);
		if (!self->configured) {
			self->configured = true;
			self->window()->updateExposure();
		}
	}
	static constexpr ext_session_lock_surface_v1_listener listener {&LockSurface::onConfigure};

	ext_session_lock_surface_v1* surface = nullptr;
	QSize size;
	bool configured = false;
};

class LockIntegration: public QtWaylandClient::QWaylandShellIntegration {
public:
	bool initialize(QtWaylandClient::QWaylandDisplay* /*display*/) override { return true; }
	QtWaylandClient::QWaylandShellSurface* createShellSurface(QtWaylandClient::QWaylandWindow* window) override {
		if (active().lock == nullptr || outputFor(window->window()->screen()) == nullptr) {
			qWarning() << "WlSessionLockSurface shown without an active lock or a real screen.";
			return nullptr;
		}
		return new LockSurface(window);
	}

	static LockIntegration* instance() {
		static auto* integration = new LockIntegration(); // NOLINT
		return integration;
	}
};

} // namespace qssl

class WlSessionLock: public QObject, public QQmlParserStatus {
	Q_OBJECT;
	Q_INTERFACES(QQmlParserStatus);
	Q_PROPERTY(bool locked READ isLocked WRITE setLocked NOTIFY lockStateChanged);
	Q_PROPERTY(bool secure READ isSecure NOTIFY secureStateChanged);
	Q_PROPERTY(QQmlComponent* surface READ surfaceComponent WRITE setSurfaceComponent NOTIFY surfaceComponentChanged);
	Q_CLASSINFO("DefaultProperty", "surface");
	QML_ELEMENT;

public:
	explicit WlSessionLock(QObject* parent = nullptr): QObject(parent) {
		QObject::connect(qGuiApp, &QGuiApplication::primaryScreenChanged, this, &WlSessionLock::onScreensChanged);
		QObject::connect(qGuiApp, &QGuiApplication::screenAdded, this, &WlSessionLock::onScreensChanged);
		QObject::connect(qGuiApp, &QGuiApplication::screenRemoved, this, &WlSessionLock::onScreensChanged);
	}

	~WlSessionLock() override {
		if (this->isLocked()) {
			qCritical() << "Session lock object was destroyed without unlocking. The session will stay "
			               "locked in case this was accidental.";
			qCritical() << "Component.onDestruction may be used if this behavior is required, but it may "
			               "result in a lock failing open.";
			// The lock proxy is left alive: destroying it after `locked` is a protocol error, and the
			// compositor keeps the session locked while (and after) this client runs.
		}
	}
	Q_DISABLE_COPY_MOVE(WlSessionLock);

	void classBegin() override {}
	void componentComplete() override {
		this->complete = true;
		this->realizeLockTarget();
	}

	[[nodiscard]] bool isLocked() const {
		const auto& active = qssl::active();
		return active.lock != nullptr && active.owner == this && !active.unlockPending;
	}
	void setLocked(bool locked) {
		if (this->lockTarget == locked) return;
		this->lockTarget = locked;
		this->realizeLockTarget();
	}

	[[nodiscard]] bool isSecure() const {
		const auto& active = qssl::active();
		return active.lock != nullptr && active.secure && !active.unlockPending;
	}

	[[nodiscard]] QQmlComponent* surfaceComponent() const { return this->mSurfaceComponent; }
	void setSurfaceComponent(QQmlComponent* component) {
		if (this->isLocked()) {
			qCritical() << "WlSessionLock.surfaceComponent cannot be changed while the lock is active.";
			return;
		}
		this->mSurfaceComponent = component;
		emit this->surfaceComponentChanged();
	}

signals:
	void lockStateChanged();
	void secureStateChanged();
	void surfaceComponentChanged();

private:
	void realizeLockTarget() {
		if (!this->complete) return;
		if (this->realizing) {
			this->awaitingRealization = true;
			return;
		}
		this->realizing = true;
		if (this->lockTarget) {
			if (qssl::manager() == nullptr) {
				qCritical() << "Cannot start session lock: The current compositor does not support the "
				               "ext-session-lock-v1 protocol.";
				this->unlock();
			} else if (this->mSurfaceComponent == nullptr) {
				qWarning() << "WlSessionLock.surface is null. Aborting lock.";
				this->unlock();
			} else {
				this->reclaimPending();
				// Surfaces are created before locking so the compositor shows a blank frame for as
				// short as possible.
				this->updateSurfaces(false);
				auto wasLocked = this->isLocked();
				if (!this->acquire()) {
					qWarning() << "Failed to acquire session lock.";
					this->unlock();
				} else {
					this->updateSurfaces(true);
					if (!wasLocked) emit this->lockStateChanged();
				}
			}
		} else {
			this->unlock();
		}
		this->realizing = false;
		if (this->awaitingRealization) {
			this->awaitingRealization = false;
			this->realizeLockTarget();
		}
	}

	bool acquire() {
		auto& active = qssl::active();
		if (active.lock != nullptr) return active.owner == this;
		active.lock = ext_session_lock_manager_v1_lock(qssl::manager());
		active.owner = this;
		active.secure = false;
		ext_session_lock_v1_add_listener(active.lock, &lockListener, nullptr);
		return true;
	}

	/// Takes back a lock this object gave up before the compositor confirmed it.
	void reclaimPending() {
		auto& active = qssl::active();
		if (active.lock == nullptr || active.owner != this || !active.unlockPending) return;
		active.unlockPending = false;
		for (auto it = active.lingering.begin(); it != active.lingering.end(); ++it) {
			if (it.value() != nullptr) this->surfaces[it.key()] = it.value();
		}
		active.lingering.clear();
	}

	static void endPending() {
		auto& active = qssl::active();
		for (const auto& surface: std::as_const(active.lingering)) {
			if (surface != nullptr) dispose(surface);
		}
		release();
		flush();
	}

	static void onLocked(void* /*data*/, ext_session_lock_v1* /*lock*/) {
		auto& active = qssl::active();
		active.secure = true;
		if (active.unlockPending) endPending();
		else if (active.owner) emit active.owner->secureStateChanged();
	}

	// The compositor refused or ended the lock: the object must now be destroyed, not unlocked.
	static void onFinished(void* /*data*/, ext_session_lock_v1* /*lock*/) {
		auto& active = qssl::active();
		auto wasSecure = active.secure;
		active.secure = false;
		if (active.unlockPending) {
			endPending();
			return;
		}
		if (active.owner && wasSecure) emit active.owner->secureStateChanged();
		if (active.owner) active.owner->unlock();
		else release();
	}

	static constexpr ext_session_lock_v1_listener lockListener {&WlSessionLock::onLocked, &WlSessionLock::onFinished};

	/// Ends the protocol lock: unlock_and_destroy once `locked` was received, destroy before
	/// that or after `finished` (the protocol makes the other order an error; upstream sends
	/// unlock_and_destroy whenever it was not finished).
	static void release() {
		auto& active = qssl::active();
		if (active.lock == nullptr) return;
		if (active.secure) ext_session_lock_v1_unlock_and_destroy(active.lock);
		else ext_session_lock_v1_destroy(active.lock);
		active = {};
	}

	static void flush() {
		if (auto* integration = QtWaylandClient::QWaylandIntegration::instance()) integration->display()->flushRequests();
	}

	void unlock() {
		this->lockTarget = false;
		auto& active = qssl::active();
		if (this->isLocked() && !active.secure) {
			active.unlockPending = true;
			for (auto it = this->surfaces.begin(); it != this->surfaces.end(); ++it) active.lingering[it.key()] = it.value();
			this->surfaces.clear();
			emit this->lockStateChanged();
			return;
		}
		for (auto* surface: std::as_const(this->surfaces)) dispose(surface);
		this->surfaces.clear();
		if (this->isLocked()) {
			release();
			flush();
			emit this->secureStateChanged();
			emit this->lockStateChanged();
		}
	}

	/// Destroys the lock surface right away (a compositor may refuse a new lock surface on an
	/// output while the old one exists), then the QML object.
	static void dispose(QObject* surface) {
		if (auto* window = surface->property("_qsLockWindow").value<QQuickWindow*>()) window->destroy();
		surface->deleteLater();
	}

	void onScreensChanged() {
		if (this->isLocked() && !this->realizing) this->updateSurfaces(true);
	}

	void updateSurfaces(bool show) {
		auto screens = QGuiApplication::screens();
		screens.removeIf([](QScreen* screen) {
			if (qssl::outputFor(screen) != nullptr) return false;
			qDebug() << "Not creating lock surface for screen" << screen << "as it is not backed by a valid wayland output.";
			return true;
		});

		for (auto it = this->surfaces.begin(); it != this->surfaces.end();) {
			if (screens.contains(it.key())) {
				++it;
			} else {
				dispose(it.value());
				it = this->surfaces.erase(it);
			}
		}

		for (auto* screen: screens) {
			if (this->surfaces.contains(screen)) continue;
			auto* context = QQmlEngine::contextForObject(this->mSurfaceComponent);
			auto* instance = this->mSurfaceComponent->create(context);
			auto* window = instance ? instance->property("_qsLockWindow").value<QQuickWindow*>() : nullptr;
			if (window == nullptr) {
				qWarning() << "WlSessionLock.surface does not create a WlSessionLockSurface. Aborting lock.";
				if (instance != nullptr) instance->deleteLater();
				this->unlock();
				return;
			}
			instance->setParent(this);
			window->setScreen(screen);
			this->surfaces[screen] = instance;
		}

		if (!show) return;
		for (auto* instance: std::as_const(this->surfaces)) {
			auto* window = instance->property("_qsLockWindow").value<QQuickWindow*>();
			if (window->isVisible()) continue;
			auto* screen = window->screen();
			window->create();
			window->setScreen(screen);
			auto* waylandWindow = dynamic_cast<QtWaylandClient::QWaylandWindow*>(window->handle());
			if (waylandWindow == nullptr) {
				qWarning() << window << "is not a wayland window. Cannot create lock surface.";
				continue;
			}
			waylandWindow->setShellIntegration(qssl::LockIntegration::instance());
			window->setVisible(true);
		}
	}

	QQmlComponent* mSurfaceComponent = nullptr;
	QMap<QScreen*, QObject*> surfaces;
	bool complete = false;
	bool lockTarget = false;
	bool realizing = false;
	bool awaitingRealization = false;
};
