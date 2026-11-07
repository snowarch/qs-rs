#pragma once
// `BackgroundEffect` attached type (upstream wayland/background_effect): asks the compositor to
// blur what is behind a window through ext-background-effect-v1. The region is set in window
// coordinates, scaled like the surface, and sent right before a frame so it commits with it.
#include <QtCore/QHash>
#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtCore/QThread>
#include <QtGui/QRegion>
#include <QtGui/QWindow>
#include <QtGui/QPlatformSurfaceEvent>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtWaylandClient/private/qwaylanddisplay_p.h>
#include <QtWaylandClient/private/qwaylandintegration_p.h>
#include <QtWaylandClient/private/qwaylandwindow_p.h>
#include <QtGui/private/qhighdpiscaling_p.h>
#include "ext-background-effect-v1-client-protocol.h"
#include "quickshell/include/window.h"

namespace qsbe {

struct Manager {
	ext_background_effect_manager_v1* manager = nullptr;
	bool blur = false;
};

inline void onCapabilities(void* data, ext_background_effect_manager_v1* /*m*/, uint32_t flags) {
	static_cast<Manager*>(data)->blur = (flags & EXT_BACKGROUND_EFFECT_MANAGER_V1_CAPABILITY_BLUR) != 0;
}

inline const ext_background_effect_manager_v1_listener listener = {onCapabilities};

// Bound once from Qt's registry on Qt's default queue; capabilities arrive with Qt's dispatch.
inline Manager* manager() {
	static Manager* manager = []() -> Manager* {
		auto* integration = QtWaylandClient::QWaylandIntegration::instance();
		if (integration == nullptr) return nullptr;
		for (const auto& global: integration->display()->globals()) {
			if (global.interface != QLatin1String(ext_background_effect_manager_v1_interface.name)) continue;
			auto* m = new Manager();
			m->manager = static_cast<ext_background_effect_manager_v1*>(
			    wl_registry_bind(global.registry, global.id, &ext_background_effect_manager_v1_interface, 1)
			);
			ext_background_effect_manager_v1_add_listener(m->manager, &listener, m);
			return m;
		}
		return nullptr;
	}();
	return manager;
}

// One effect object per wl_surface, ever: asking for a second one is a protocol error that
// kills the connection (10-20 12:37, live: a reload whose old generation never let go of a
// retaken window's effect). Every BackgroundEffect on a surface joins its list; the newest one
// holds the object (sends its region, and destroys it when nobody is left). When the holder lets
// go, the one before it takes the object back and sends its own region again.
struct Effect {
	ext_background_effect_surface_v1* object = nullptr;
	QList<QObject*> holders;
};

inline QHash<wl_surface*, Effect>& effects() {
	static QHash<wl_surface*, Effect> map;
	return map;
}

inline ext_background_effect_surface_v1* take(Manager* manager, wl_surface* surface, QObject* holder) {
	auto& e = effects()[surface];
	if (e.object == nullptr) e.object = ext_background_effect_manager_v1_get_background_effect(manager->manager, surface);
	e.holders.removeAll(holder);
	e.holders.append(holder);
	return e.object;
}

inline bool holds(wl_surface* surface, const QObject* holder) {
	auto it = effects().constFind(surface);
	return it != effects().constEnd() && !it->holders.isEmpty() && it->holders.last() == holder;
}

// `gone`: the wl_surface is going away, and its effect with it whoever holds it (an entry left
// behind would hand a dead object to the next surface at the same address).
inline void release(wl_surface* surface, QObject* holder, bool gone = false) {
	auto it = effects().find(surface);
	if (it == effects().end()) return;
	bool held = !it->holders.isEmpty() && it->holders.last() == holder;
	it->holders.removeAll(holder);
	if (gone || it->holders.isEmpty()) {
		ext_background_effect_surface_v1_destroy(it->object);
		effects().erase(it);
	} else if (held) {
		QMetaObject::invokeMethod(it->holders.last(), "updateBlurRegion", Qt::QueuedConnection);
	}
}

} // namespace qsbe

class BackgroundEffect: public QObject {
	Q_OBJECT;
	// The region of the window to blur. Without one, nothing is blurred.
	Q_PROPERTY(PendingRegion* blurRegion READ blurRegion WRITE setBlurRegion NOTIFY blurRegionChanged);
	QML_ELEMENT;
	QML_UNCREATABLE("BackgroundEffect can only be used as an attached object.");
	QML_ATTACHED(BackgroundEffect);

public:
	// Attached to a Quickshell window object (its backing QQuickWindow is `_backingWindow`).
	static BackgroundEffect* qmlAttachedProperties(QObject* object) {
		if (!object->property("_backingWindow").isValid()) return nullptr;
		return new BackgroundEffect(object);
	}

	explicit BackgroundEffect(QObject* proxy): QObject(nullptr), proxy(proxy) {
		QObject::connect(proxy, &QObject::destroyed, this, &BackgroundEffect::onProxyDestroyed);
		// The proxy gets its backing window once complete (maybe one reused from before a reload).
		auto* mo = proxy->metaObject();
		auto notify = mo->property(mo->indexOfProperty("_backingWindow")).notifySignal();
		auto slot = this->metaObject()->method(this->metaObject()->indexOfMethod("onBackingWindowChanged()"));
		if (notify.isValid()) QObject::connect(proxy, notify, this, slot);
		this->onBackingWindowChanged();
	}

	Q_INVOKABLE void onBackingWindowChanged() {
		auto* window = this->proxy ? this->proxy->property("_backingWindow").value<QQuickWindow*>() : nullptr;
		if (window == this->window) return;
		if (this->window) {
			this->destroySurface();
			this->window->removeEventFilter(this);
			QObject::disconnect(this->window, nullptr, this, nullptr);
			if (this->waylandWindow) QObject::disconnect(this->waylandWindow, nullptr, this, nullptr);
			this->waylandWindow = nullptr;
		}
		this->window = window;
		if (window == nullptr) return;
		window->installEventFilter(this);
		QObject::connect(window, &QWindow::visibleChanged, this, &BackgroundEffect::onWindowVisibleChanged);
		QObject::connect(window, &QQuickWindow::afterAnimating, this, &BackgroundEffect::onBeforeFrame);
		// Where the scene graph syncs on this thread (qs-rs's shared basic loop) the region is sent there, after every
		// afterAnimating handler: a table published from one (the iRiS Island flushes its bodies there) is in that frame.
		QObject::connect(window, &QQuickWindow::beforeSynchronizing, this, &BackgroundEffect::onSync, Qt::DirectConnection);
		QObject::connect(window, &QWindow::screenChanged, this, &BackgroundEffect::updateBlurRegion);
		this->onWindowVisibleChanged();
	}

	~BackgroundEffect() override {
		if (this->surface) qsbe::release(this->wlSurface, this);
	}

	[[nodiscard]] PendingRegion* blurRegion() const { return this->mBlurRegion; }

	void setBlurRegion(PendingRegion* region) {
		if (region == this->mBlurRegion) return;
		if (this->mBlurRegion) QObject::disconnect(this->mBlurRegion, nullptr, this, nullptr);
		this->mBlurRegion = region;
		if (region) {
			QObject::connect(region, &QObject::destroyed, this, &BackgroundEffect::onBlurRegionDestroyed);
			QObject::connect(region, &PendingRegion::changed, this, &BackgroundEffect::updateBlurRegion);
		}
		this->updateBlurRegion();
		emit this->blurRegionChanged();
	}

signals:
	void blurRegionChanged();

protected:
	bool eventFilter(QObject* object, QEvent* event) override {
		if (event->type() == QEvent::PlatformSurface) {
			auto* surfaceEvent = static_cast<QPlatformSurfaceEvent*>(event);
			if (surfaceEvent->surfaceEventType() == QPlatformSurfaceEvent::SurfaceAboutToBeDestroyed) {
				this->destroySurface(true);
			}
		} else if (event->type() == QEvent::Resize) {
			this->updateBlurRegion();
		}
		return QObject::eventFilter(object, event);
	}

private:
	void onBlurRegionDestroyed() {
		this->mBlurRegion = nullptr;
		this->updateBlurRegion();
		emit this->blurRegionChanged();
	}

	// Also called when the effect object comes back to this one (qsbe::release).
	Q_INVOKABLE void updateBlurRegion() {
		if (this->surface == nullptr && this->wlSurface == nullptr && this->waylandWindow != nullptr
		    && this->waylandWindow->wlSurface() != nullptr && qsbe::manager() == nullptr)
			this->warnUnsupported();
		if (this->surface == nullptr || this->window == nullptr || !qsbe::holds(this->wlSurface, this)) return;
		this->pending = true;
		this->window->update();
	}

	// GUI thread, after animations and before the scene graph sync of the next frame: the
	// region request is double buffered and commits with that frame's wl_surface.commit.
	void onBeforeFrame() {
		if (this->syncsHere) return;
		this->send();
	}

	void onSync() {
		if (QThread::currentThread() != this->thread()) return;
		this->syncsHere = true;
		this->send();
	}

	void send() {
		if (this->surface == nullptr || !this->pending) return;
		this->pending = false;
		if (!qsbe::holds(this->wlSurface, this)) return;
		if (this->waylandWindow == nullptr || this->waylandWindow->wlSurface() == nullptr) return;
		QRegion region;
		if (this->mBlurRegion) {
			region = this->mBlurRegion->applyTo(QRect(0, 0, this->window->width(), this->window->height()));
			auto scale = QHighDpiScaling::factor(static_cast<QWindow*>(this->window.data()));
			if (!qFuzzyCompare(scale, 1.0)) region = QHighDpi::scale(region, scale);
			auto margins = this->waylandWindow->clientSideMargins();
			region.translate(margins.left(), margins.top());
		}
		if (region.isEmpty()) {
			ext_background_effect_surface_v1_set_blur_region(this->surface, nullptr);
		} else {
			auto* wlRegion = this->waylandWindow->display()->createRegion(region);
			ext_background_effect_surface_v1_set_blur_region(this->surface, wlRegion);
			wl_region_destroy(wlRegion);
		}
	}

	void onWindowVisibleChanged() {
		if (this->window->isVisible() && this->window->handle() == nullptr) this->window->create();
		auto* ww = dynamic_cast<QtWaylandClient::QWaylandWindow*>(this->window->handle());
		if (ww == this->waylandWindow) return;
		if (this->waylandWindow) QObject::disconnect(this->waylandWindow, nullptr, this, nullptr);
		this->waylandWindow = ww;
		if (ww == nullptr) return;
		QObject::connect(ww, &QtWaylandClient::QWaylandWindow::surfaceCreated, this, &BackgroundEffect::onSurfaceCreated);
		QObject::connect(ww, &QtWaylandClient::QWaylandWindow::surfaceDestroyed, this, [this] { this->destroySurface(true); });
		if (ww->wlSurface() != nullptr) this->onSurfaceCreated();
	}

	// Once per process, and only when a region is actually asked for: a window that merely carries
	// the attached object (its binding gave null) asks the compositor for nothing.
	void warnUnsupported() {
		static bool warned = false;
		if (warned || this->mBlurRegion == nullptr) return;
		warned = true;
		qWarning() << "Cannot enable background effect as ext-background-effect-v1 is not supported by the current compositor.";
	}

	void onSurfaceCreated() {
		auto* manager = qsbe::manager();
		if (manager == nullptr) {
			this->warnUnsupported();
			return;
		}
		auto* wl = this->waylandWindow->wlSurface();
		if (this->surface && this->wlSurface != wl) qsbe::release(this->wlSurface, this);
		this->wlSurface = wl;
		this->surface = qsbe::take(manager, wl, this);
		this->pending = this->mBlurRegion != nullptr;
		if (this->pending) this->window->update();
	}

	void destroySurface(bool gone = false) {
		if (this->surface) qsbe::release(this->wlSurface, this, gone);
		this->surface = nullptr;
		this->wlSurface = nullptr;
		this->pending = false;
		if (this->proxy == nullptr) this->deleteLater();
	}

	void onProxyDestroyed() {
		// Kept until the wl_surface goes away: destroying the effect first would show one frame
		// without blur before the surface is destroyed. A window kept for the next generation
		// keeps its surface, frozen until the new proxy's effect is set: release ours now.
		this->proxy = nullptr;
		if (this->window && this->window->property("_qsStashed").toBool()) this->destroySurface();
		else if (this->surface == nullptr) this->deleteLater();
	}

	QPointer<QObject> proxy;
	QPointer<QQuickWindow> window;
	QPointer<QtWaylandClient::QWaylandWindow> waylandWindow;
	PendingRegion* mBlurRegion = nullptr;
	ext_background_effect_surface_v1* surface = nullptr;
	wl_surface* wlSurface = nullptr;
	bool pending = false;
	bool syncsHere = false;
};
