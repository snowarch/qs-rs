#pragma once
// Window glue that needs QML's attached-property machinery or QWindow API that cxx-qt cannot
// express: QsWindow attached lookup and input masks.
#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtCore/QDebug>
#include <QtGui/QRegion>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtGui/QWindow>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include "windowpool.h"
#include <QtQml/QQmlListProperty>
#include <cmath>
#include <QtGui/QOffscreenSurface>
#include <QtGui/rhi/qrhi.h>
#include <QtQuick/QQuickGraphicsDevice>

// One QRhi (one GL context) for every window instead of one per window: each Mesa context costs
// heap, VRAM and driver threads. The host runs the basic render loop for it (a QRhi is used from one
// thread). QSRS_PER_WINDOW_GL=1 restores upstream's per-window contexts and threaded loop.
inline bool qsSharedGraphics() {
	static const bool shared = !qEnvironmentVariableIsSet("QSRS_PER_WINDOW_GL");
	return shared;
}

// The format every window and the shared context use: Qt Quick's own (depth 24, stencil 8) with
// alpha, so one EGL config fits all surfaces. Swap interval 0: with every window on the GUI thread,
// a swap must not wait for a frame callback the compositor may hold back for a covered surface.
inline QSurfaceFormat qsSharedFormat() {
	auto format = QSurfaceFormat::defaultFormat();
	format.setDepthBufferSize(24);
	format.setStencilBufferSize(8);
	format.setAlphaBufferSize(8);
	format.setSwapInterval(0);
	return format;
}

inline QRhi* qsSharedRhi() {
	static QRhi* rhi = [] {
		QRhiGles2InitParams params;
		params.format = qsSharedFormat();
		params.fallbackSurface = QRhiGles2InitParams::newFallbackSurface(params.format);
		return QRhi::create(QRhi::OpenGLES2, &params);
	}();
	return rhi;
}

namespace ExclusionMode {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Normal = 0, Ignore = 1, Auto = 2 };
Q_ENUM_NS(Enum);
} // namespace ExclusionMode

// PanelWindow's exclusiveZone/exclusionMode pair, aliased by PanelWindow.qml. Needs a real setter:
// as in quickshell, every write of exclusiveZone (even of the same value) switches the mode to
// Normal, so `exclusionMode: Ignore` next to an `exclusiveZone` binding still reserves the zone.
class QsPanelExclusion: public QObject {
	Q_OBJECT;
	Q_PROPERTY(qint32 exclusiveZone READ exclusiveZone WRITE setExclusiveZone NOTIFY exclusiveZoneChanged);
	Q_PROPERTY(ExclusionMode::Enum exclusionMode READ exclusionMode WRITE setExclusionMode NOTIFY exclusionModeChanged);
	QML_ELEMENT;

public:
	explicit QsPanelExclusion(QObject* parent = nullptr): QObject(parent) {}

	[[nodiscard]] qint32 exclusiveZone() const { return this->mZone; }
	void setExclusiveZone(qint32 zone) {
		const bool changed = zone != this->mZone;
		this->mZone = zone;
		this->setExclusionMode(ExclusionMode::Normal);
		if (changed) emit this->exclusiveZoneChanged();
	}

	[[nodiscard]] ExclusionMode::Enum exclusionMode() const { return this->mMode; }
	void setExclusionMode(ExclusionMode::Enum mode) {
		if (mode == this->mMode) return;
		this->mMode = mode;
		emit this->exclusionModeChanged();
	}

signals:
	void exclusiveZoneChanged();
	void exclusionModeChanged();

private:
	qint32 mZone = 0;
	ExclusionMode::Enum mMode = ExclusionMode::Auto;
};

namespace Edges {
Q_NAMESPACE;
QML_NAMED_ELEMENT(Edges);
enum Enum : quint8 { None = 0, Top = Qt::TopEdge, Left = Qt::LeftEdge, Right = Qt::RightEdge, Bottom = Qt::BottomEdge };
Q_ENUM_NS(Enum);
} // namespace Edges

// Same bits as xdg_positioner's constraint_adjustment.
namespace PopupAdjustment {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 {
	None = 0,
	SlideX = 1,
	SlideY = 2,
	Slide = SlideX | SlideY,
	FlipX = 4,
	FlipY = 8,
	Flip = FlipX | FlipY,
	ResizeX = 16,
	ResizeY = 32,
	Resize = ResizeX | ResizeY,
	All = Slide | Flip | Resize,
};
Q_ENUM_NS(Enum);
} // namespace PopupAdjustment

namespace RegionShape {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Rect = 0, Ellipse = 1 };
Q_ENUM_NS(Enum);
} // namespace RegionShape

namespace Intersection {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Combine = 0, Subtract = 1, Intersect = 2, Xor = 3 };
Q_ENUM_NS(Enum);
} // namespace Intersection

// `Region`: a shape (rect/ellipse, from an item or explicit geometry) combined with child regions.
// Same semantics as quickshell's PendingRegion. Emits changed() whenever the result may differ.
class PendingRegion: public QObject {
	Q_OBJECT;
	Q_PROPERTY(RegionShape::Enum shape MEMBER mShape NOTIFY changed);
	Q_PROPERTY(Intersection::Enum intersection MEMBER mIntersection NOTIFY changed);
	Q_PROPERTY(QQuickItem* item READ item WRITE setItem NOTIFY itemChanged);
	Q_PROPERTY(qint32 x MEMBER mX NOTIFY changed);
	Q_PROPERTY(qint32 y MEMBER mY NOTIFY changed);
	Q_PROPERTY(qint32 width MEMBER mWidth NOTIFY changed);
	Q_PROPERTY(qint32 height MEMBER mHeight NOTIFY changed);
	Q_PROPERTY(qint32 radius MEMBER mRadius NOTIFY changed);
	Q_PROPERTY(qint32 topLeftRadius READ topLeftRadius WRITE setTopLeftRadius NOTIFY changed);
	Q_PROPERTY(qint32 topRightRadius READ topRightRadius WRITE setTopRightRadius NOTIFY changed);
	Q_PROPERTY(qint32 bottomLeftRadius READ bottomLeftRadius WRITE setBottomLeftRadius NOTIFY changed);
	Q_PROPERTY(qint32 bottomRightRadius READ bottomRightRadius WRITE setBottomRightRadius NOTIFY changed);
	Q_PROPERTY(QQmlListProperty<PendingRegion> regions READ regions);
	Q_CLASSINFO("DefaultProperty", "regions");
	QML_NAMED_ELEMENT(Region);

public:
	explicit PendingRegion(QObject* parent = nullptr): QObject(parent) {}

	[[nodiscard]] QQuickItem* item() const { return this->mItem; }
	void setItem(QQuickItem* item) {
		if (item == this->mItem) return;
		this->unwatch();
		this->mItem = item;
		this->watch();
		emit this->itemChanged();
		emit this->changed();
	}

	qint32 topLeftRadius() const { return this->mTl >= 0 ? this->mTl : this->mRadius; }
	qint32 topRightRadius() const { return this->mTr >= 0 ? this->mTr : this->mRadius; }
	qint32 bottomLeftRadius() const { return this->mBl >= 0 ? this->mBl : this->mRadius; }
	qint32 bottomRightRadius() const { return this->mBr >= 0 ? this->mBr : this->mRadius; }
	void setTopLeftRadius(qint32 r) { this->mTl = r; emit this->changed(); }
	void setTopRightRadius(qint32 r) { this->mTr = r; emit this->changed(); }
	void setBottomLeftRadius(qint32 r) { this->mBl = r; emit this->changed(); }
	void setBottomRightRadius(qint32 r) { this->mBr = r; emit this->changed(); }

	QQmlListProperty<PendingRegion> regions() {
		return QQmlListProperty<PendingRegion>(this, nullptr, &PendingRegion::regionsAppend, &PendingRegion::regionsCount, &PendingRegion::regionsAt, &PendingRegion::regionsClear);
	}

	[[nodiscard]] bool empty() const { return this->mItem == nullptr && this->mWidth == 0 && this->mHeight == 0; }

	// Virtual: FieldRegion (fieldregion.h) rasterises its own shape.
	[[nodiscard]] virtual QRegion build() const {
		auto type = this->mShape == RegionShape::Ellipse ? QRegion::Ellipse : QRegion::Rectangle;
		QRegion region;
		if (this->empty()) {
		} else if (this->mItem != nullptr) {
			auto origin = this->mItem->mapToScene(QPointF(0, 0));
			auto extent = this->mItem->mapToScene(QPointF(this->mItem->width(), this->mItem->height()));
			auto size = extent - origin;
			region = QRegion(int(origin.x()), int(origin.y()), int(std::ceil(size.x())), int(std::ceil(size.y())), type);
		} else {
			region = QRegion(this->mX, this->mY, this->mWidth, this->mHeight, type);
		}
		if (this->mShape == RegionShape::Rect && !region.isEmpty()) {
			auto tl = std::max(this->topLeftRadius(), 0), tr = std::max(this->topRightRadius(), 0);
			auto bl = std::max(this->bottomLeftRadius(), 0), br = std::max(this->bottomRightRadius(), 0);
			if (tl > 0 || tr > 0 || bl > 0 || br > 0) {
				auto r = region.boundingRect();
				auto x = r.x(), y = r.y(), w = r.width(), h = r.height();
				auto topS = tl + tr > w ? double(w) / (tl + tr) : 1.0;
				auto botS = bl + br > w ? double(w) / (bl + br) : 1.0;
				auto leftS = tl + bl > h ? double(h) / (tl + bl) : 1.0;
				auto rightS = tr + br > h ? double(h) / (tr + br) : 1.0;
				tl = qint32(tl * std::min(topS, leftS));
				tr = qint32(tr * std::min(topS, rightS));
				bl = qint32(bl * std::min(botS, leftS));
				br = qint32(br * std::min(botS, rightS));
				auto cut = [&](int bx, int by, int r, int ex, int ey) {
					auto box = QRegion(bx, by, r, r);
					region -= box - (QRegion(ex, ey, r * 2, r * 2, QRegion::Ellipse) & box);
				};
				if (tl > 0) cut(x, y, tl, x, y);
				if (tr > 0) cut(x + w - tr, y, tr, x + w - tr * 2, y);
				if (bl > 0) cut(x, y + h - bl, bl, x, y + h - bl * 2);
				if (br > 0) cut(x + w - br, y + h - br, br, x + w - br * 2, y + h - br * 2);
			}
		}
		for (auto* child: this->mRegions) region = child->applyTo(region);
		return region;
	}

	[[nodiscard]] QRegion applyTo(const QRegion& region) const {
		switch (this->mIntersection) {
		case Intersection::Combine: return region.united(this->build());
		case Intersection::Subtract: return region.subtracted(this->build());
		case Intersection::Intersect: return region.intersected(this->build());
		case Intersection::Xor: return region.xored(this->build());
		}
		return region;
	}

	// Against a window rect the default (Combine) keeps the region as is instead of uniting it
	// with the whole rect, so an empty `Region {}` mask makes a window click-through.
	[[nodiscard]] QRegion applyTo(const QRect& rect) const {
		if (this->mIntersection == Intersection::Combine) return this->build();
		return this->applyTo(QRegion(rect));
	}

signals:
	void changed();
	void itemChanged();

private:
	// Geometry of the item and every ancestor affects mapToScene().
	void watch() {
		for (auto* i = this->mItem; i != nullptr; i = i->parentItem()) {
			for (auto sig: {&QQuickItem::xChanged, &QQuickItem::yChanged, &QQuickItem::widthChanged, &QQuickItem::heightChanged}) {
				this->conns << QObject::connect(i, sig, this, &PendingRegion::changed);
			}
			this->conns << QObject::connect(i, &QQuickItem::parentChanged, this, [this] { this->unwatch(); this->watch(); emit this->changed(); });
		}
		if (this->mItem) this->conns << QObject::connect(this->mItem, &QObject::destroyed, this, [this] { this->conns.clear(); this->mItem = nullptr; emit this->itemChanged(); emit this->changed(); });
	}
	void unwatch() {
		for (auto& c: this->conns) QObject::disconnect(c);
		this->conns.clear();
	}

	static void regionsAppend(QQmlListProperty<PendingRegion>* p, PendingRegion* r) {
		auto* self = static_cast<PendingRegion*>(p->object);
		if (!r) return;
		self->mRegions.append(r);
		QObject::connect(r, &PendingRegion::changed, self, &PendingRegion::changed);
		QObject::connect(r, &QObject::destroyed, self, [self, r] { self->mRegions.removeAll(r); emit self->changed(); });
		emit self->changed();
	}
	static qsizetype regionsCount(QQmlListProperty<PendingRegion>* p) { return static_cast<PendingRegion*>(p->object)->mRegions.size(); }
	static PendingRegion* regionsAt(QQmlListProperty<PendingRegion>* p, qsizetype i) { return static_cast<PendingRegion*>(p->object)->mRegions.at(i); }
	static void regionsClear(QQmlListProperty<PendingRegion>* p) {
		auto* self = static_cast<PendingRegion*>(p->object);
		for (auto* r: self->mRegions) QObject::disconnect(r, nullptr, self, nullptr);
		self->mRegions.clear();
		emit self->changed();
	}

	RegionShape::Enum mShape = RegionShape::Rect;
	Intersection::Enum mIntersection = Intersection::Combine;
	QQuickItem* mItem = nullptr;
	qint32 mX = 0, mY = 0, mWidth = 0, mHeight = 0, mRadius = 0;
	qint32 mTl = -1, mTr = -1, mBl = -1, mBr = -1;
	QList<PendingRegion*> mRegions;
	QList<QMetaObject::Connection> conns;
};

// Helpers the QML window types call on their backing QQuickWindow.
namespace qsrs {
// src/cpp/windowprivate.cpp. Returns whether it changed.
bool setWindowUpdatesEnabled(QQuickWindow* window, bool enabled);
}

class QsWindowSupport: public QObject {
	Q_OBJECT;
	QML_ELEMENT;
	QML_SINGLETON;

public:
	// Input mask: the region applied to the full window rect; a mask that ends up empty makes
	// the window transparent for input, a null mask restores normal input.
	Q_INVOKABLE void applyMask(QQuickWindow* window, PendingRegion* mask) {
		if (!window) return;
		QRegion region;
		if (mask) region = mask->applyTo(QRect(0, 0, window->width(), window->height()));
		window->setFlag(Qt::WindowTransparentForInput, mask != nullptr && region.isEmpty());
		window->setMask(region);
	}

	// Bounding rect of a window's input mask as "x,y wxh" (tests).
	Q_INVOKABLE QString maskRect(QWindow* window) {
		if (!window) return {};
		auto r = window->mask().boundingRect();
		return QString("%1,%2 %3x%4").arg(r.x()).arg(r.y()).arg(r.width()).arg(r.height());
	}

	// Frozen windows of the previous generation still waiting for a claim (tests).
	Q_INVOKABLE int stashedWindows() {
		int count = 0;
		for (auto& list: QsWindowPool::stashed()) {
			for (auto& window: list) count += window ? 1 : 0;
		}
		return count;
	}

	// The backing window of a proxy: a stashed one with the same key, or a new one. Owned by the
	// proxy (QObject child), with `content` as the proxy's content item inside it.
	Q_INVOKABLE QQuickWindow* acquireWindow(QObject* proxy, QQuickItem* content, const QString& tag, const QString& screen) {
		if (!proxy) return nullptr;
		QString key;
		if (!tag.isEmpty()) {
			auto* ctx = qmlContext(proxy);
			key = (ctx ? ctx->baseUrl().toString() : QString()) + '|' + tag + '|' + screen + '|' + QsWindowPool::treePath(proxy);
		}
		auto* window = key.isEmpty() ? nullptr : QsWindowPool::take(key);
		bool reused = window != nullptr;
		if (!reused) {
			window = new QQuickWindow();
			QsWindowPool::live().append(window);
			QObject::connect(window, &QWindow::visibleChanged, window, [window](bool visible) {
				if (visible) QsWindowPool::yieldTo(window);
			});
		}
		QQmlEngine::setObjectOwnership(window, QQmlEngine::CppOwnership);
		static_cast<QObject*>(window)->setParent(proxy);
		window->setProperty("_qsReuseKey", key);
		if (content) content->setParentItem(window->contentItem());
		if (reused) {
			// Stays frozen on the old frame until the proxy shows it (thawWindow): shells often
			// start a window hidden until their config loads, which would unmap it meanwhile.
			window->setProperty("_qsWindow", QVariant::fromValue(proxy));
		} else {
			this->registerProxy(window, proxy);
		}
		return window;
	}

	// Upstream's ProxyWindowBase::setUpdatesEnabled: the render loop skips a window whose private
	// updatesEnabled is off, so it keeps its last frame on screen and costs nothing.
	Q_INVOKABLE void setUpdatesEnabled(QQuickWindow* window, bool enabled) {
		if (window && qsrs::setWindowUpdatesEnabled(window, enabled) && enabled) window->update();
	}

	Q_INVOKABLE bool isFrozen(QQuickWindow* window) { return window && window->property("_qsFrozen").toBool(); }

	Q_INVOKABLE void thawWindow(QQuickWindow* window) {
		if (!window || !this->isFrozen(window)) return;
		window->setProperty("_qsFrozen", false);
		window->removeEventFilter(QsWindowFreeze::instance());
		// Expose and update requests were dropped while frozen: draw the new content.
		window->update();
	}

	// QWindow::screen is not a QML property of a plain QQuickWindow. QML hands screens around as
	// QQuickScreenInfo (Qt.application.screens, ShellScreen._info): matched by name.
	Q_INVOKABLE void setWindowScreen(QWindow* window, QObject* info) {
		if (!window || !info) return;
		auto* screen = qobject_cast<QScreen*>(info);
		if (!screen) {
			auto name = info->property("name").toString();
			for (auto* s: QGuiApplication::screens()) {
				if (s->name() == name) screen = s;
			}
		}
		if (screen && window->screen() != screen) window->setScreen(screen);
	}

	Q_INVOKABLE QString windowScreenName(QWindow* window) {
		auto* screen = window ? window->screen() : nullptr;
		return screen ? screen->name() : QString();
	}

	Q_INVOKABLE void registerProxy(QQuickWindow* window, QObject* proxy) {
		if (!window) return;
		window->setProperty("_qsWindow", QVariant::fromValue(proxy));
		if (qsSharedGraphics()) {
			if (auto* rhi = qsSharedRhi()) {
				window->setFormat(qsSharedFormat());
				window->setGraphicsDevice(QQuickGraphicsDevice::fromRhi(rhi));
			}
		}
	}

	// Variants instances, as upstream creates them (variants.cpp:154-167): no QML parent while
	// created (an Item delegate under a QtObject is "not placed in the graphics scene"), then
	// owned by `owner` as a plain QObject child.
	Q_INVOKABLE QObject* createOwned(QQmlComponent* component, QObject* owner, const QVariantMap& props) {
		if (!component) return nullptr;
		auto* instance =
		    component->createWithInitialProperties(props, QQmlEngine::contextForObject(component));
		if (!instance) {
			qWarning() << component->errorString().toStdString().c_str();
			return nullptr;
		}
		QQmlEngine::setObjectOwnership(instance, QQmlEngine::CppOwnership);
		instance->setParent(owner);
		return instance;
	}

	// `url` resolved against the file that instantiated `object` (upstream's
	// qmlContext(this)->resolvedUrl for relative paths like LazyLoader.source).
	// JS cannot destroy() a C++-owned object; createOwned instances go through here.
	Q_INVOKABLE void destroyOwned(QObject* instance) {
		if (instance) instance->deleteLater();
	}

	Q_INVOKABLE QUrl resolvedUrl(QObject* object, const QUrl& url) {
		auto* ctx = object ? qmlContext(object) : nullptr;
		return ctx ? ctx->resolvedUrl(url) : url;
	}

	// The Quickshell window object that owns an item's window, or null.
	Q_INVOKABLE QObject* proxyOf(QQuickItem* item) {
		auto* win = item ? item->window() : nullptr;
		return win ? win->property("_qsWindow").value<QObject*>() : nullptr;
	}

	// Popup placement for QtWayland's xdg_positioner (read when the popup surface is created):
	// anchor rect in parent window coordinates, anchor edges, gravity, constraint adjustment.
	Q_INVOKABLE void setPopupAnchor(QWindow* popup, QRect rect, int edges, int gravity, int adjustment) {
		if (!popup) return;
		popup->setProperty("_q_waylandPopupAnchorRect", rect);
		popup->setProperty("_q_waylandPopupAnchor", QVariant::fromValue(Qt::Edges(edges)));
		popup->setProperty("_q_waylandPopupGravity", QVariant::fromValue(Qt::Edges(gravity)));
		popup->setProperty("_q_waylandPopupConstraintAdjustment", static_cast<uint>(adjustment));
	}
};

// PopupAnchor.rect and .margins: upstream's Box and Margins values, as objects so inir's grouped
// bindings (`anchor.rect.x: ...`) and writes (`anchor.rect.y = ...`) keep working.
class QsPopupBox: public QObject {
	Q_OBJECT;
	QML_ANONYMOUS;
	Q_PROPERTY(int x MEMBER x NOTIFY changed);
	Q_PROPERTY(int y MEMBER y NOTIFY changed);
	Q_PROPERTY(int width MEMBER width NOTIFY changed);
	Q_PROPERTY(int height MEMBER height NOTIFY changed);

public:
	using QObject::QObject;
	int x = 0, y = 0, width = 0, height = 0;
	[[nodiscard]] QRect qrect() const { return {x, y, width, height}; }

signals:
	void changed();
};

class QsPopupMargins: public QObject {
	Q_OBJECT;
	QML_ANONYMOUS;
	Q_PROPERTY(int left MEMBER left NOTIFY changed);
	Q_PROPERTY(int right MEMBER right NOTIFY changed);
	Q_PROPERTY(int top MEMBER top NOTIFY changed);
	Q_PROPERTY(int bottom MEMBER bottom NOTIFY changed);

public:
	using QObject::QObject;
	int left = 0, right = 0, top = 0, bottom = 0;
	[[nodiscard]] QMargins qmargins() const { return {left, top, right, bottom}; }

signals:
	void changed();
};

// Where a PopupWindow attaches, ported from upstream core/popupanchor.cpp: guarded setters, no
// bindings. With an item set, `window` follows the Quickshell window the item lives in (upstream
// reads it the same way), so a QML binding never has to chase it.
class QsPopupAnchor: public QObject {
	Q_OBJECT;
	QML_NAMED_ELEMENT(PopupAnchor);
	Q_PROPERTY(QObject* window READ window WRITE setWindow NOTIFY windowChanged);
	Q_PROPERTY(QQuickItem* item READ item WRITE setItem NOTIFY itemChanged);
	Q_PROPERTY(QsPopupBox* rect READ rect CONSTANT);
	Q_PROPERTY(QsPopupMargins* margins READ margins CONSTANT);
	Q_PROPERTY(int edges READ edges WRITE setEdges NOTIFY edgesChanged);
	Q_PROPERTY(int gravity READ gravity WRITE setGravity NOTIFY gravityChanged);
	Q_PROPERTY(int adjustment READ adjustment WRITE setAdjustment NOTIFY adjustmentChanged);
	// Anchor rect in the window's content coordinates, what xdg_positioner gets.
	Q_PROPERTY(QRect _windowRect READ windowRect NOTIFY windowRectChanged);

public:
	explicit QsPopupAnchor(QObject* parent = nullptr): QObject(parent) {
		QObject::connect(&mRect, &QsPopupBox::changed, this, &QsPopupAnchor::userRectChanged);
		QObject::connect(&mMargins, &QsPopupMargins::changed, this, &QsPopupAnchor::userRectChanged);
	}

	[[nodiscard]] QObject* window() const { return mWindow; }
	void setWindow(QObject* window) {
		setItem(nullptr);
		setWindowInternal(window);
	}

	[[nodiscard]] QQuickItem* item() const { return mItem; }
	void setItem(QQuickItem* item) {
		if (item == mItem) return;
		if (mItem) QObject::disconnect(mItem, nullptr, this, nullptr);
		watchBacking(nullptr);
		mItem = item;
		if (item) {
			QObject::connect(item, &QObject::destroyed, this, [this] {
				mItem = nullptr;
				watchBacking(nullptr);
				emit itemChanged();
				setWindowInternal(nullptr);
			});
			QObject::connect(item, &QQuickItem::windowChanged, this, &QsPopupAnchor::onItemWindowChanged);
		}
		onItemWindowChanged();
		emit itemChanged();
	}

	QsPopupBox* rect() { return &mRect; }
	QsPopupMargins* margins() { return &mMargins; }

	[[nodiscard]] int edges() const { return mEdges; }
	void setEdges(int edges) {
		if (edges == mEdges) return;
		if (opposing(edges)) {
			qWarning() << "Cannot set opposing edges for anchor edges. Tried to set" << edges;
			return;
		}
		mEdges = edges;
		emit edgesChanged();
	}

	[[nodiscard]] int gravity() const { return mGravity; }
	void setGravity(int gravity) {
		if (gravity == mGravity) return;
		if (opposing(gravity)) {
			qWarning() << "Cannot set opposing edges for anchor gravity. Tried to set" << gravity;
			return;
		}
		mGravity = gravity;
		emit gravityChanged();
	}

	[[nodiscard]] int adjustment() const { return mAdjustment; }
	void setAdjustment(int adjustment) {
		if (adjustment == mAdjustment) return;
		mAdjustment = adjustment;
		emit adjustmentChanged();
	}

	[[nodiscard]] QRect windowRect() const { return mWindowRect; }

	// Re-maps an item anchor into its window, then lets the shell adjust (`anchoring`).
	Q_INVOKABLE void updateAnchor() {
		mapItemRect();
		emit anchoring();
	}

signals:
	void windowChanged();
	void itemChanged();
	void edgesChanged();
	void gravityChanged();
	void adjustmentChanged();
	void windowRectChanged();
	void anchoring();

protected:
	// A window registers its Quickshell object ("_qsWindow") when it completes, which can be
	// after the item entered it: the dynamic property change is the cue to resolve again.
	bool eventFilter(QObject* watched, QEvent* event) override {
		if (watched == mBacking && event->type() == QEvent::DynamicPropertyChange
		    && static_cast<QDynamicPropertyChangeEvent*>(event)->propertyName() == "_qsWindow")
		{
			onItemWindowChanged();
		}
		return false;
	}

private:
	static bool opposing(int e) {
		return ((e & Qt::TopEdge) && (e & Qt::BottomEdge)) || ((e & Qt::LeftEdge) && (e & Qt::RightEdge));
	}

	// With an item the rect is the item's: it lands mapped, or a resize while shown re-places the popup twice, the
	// second time from inside the first (a binding loop on PopupWindow._placement).
	void userRectChanged() {
		if (!mapItemRect()) setWindowRect(mRect.qrect().marginsRemoved(mMargins.qmargins()));
	}

	bool mapItemRect() {
		auto* content = mWindow ? mWindow->property("contentItem").value<QQuickItem*>() : nullptr;
		if (!mItem || !content) return false;
		auto user = mRect.qrect();
		auto base = user.width() == 0 && user.height() == 0 ? mItem->boundingRect() : QRectF(user);
		setWindowRect(content->mapFromItem(mItem, base.marginsRemoved(mMargins.qmargins())).toRect());
		return true;
	}

	void setWindowRect(QRect rect) {
		if (rect.width() <= 0) rect.setWidth(1);
		if (rect.height() <= 0) rect.setHeight(1);
		if (rect == mWindowRect) return;
		mWindowRect = rect;
		emit windowRectChanged();
	}

	void watchBacking(QQuickWindow* backing) {
		if (backing == mBacking) return;
		if (mBacking) mBacking->removeEventFilter(this);
		mBacking = backing;
		if (backing) backing->installEventFilter(this);
	}

	void onItemWindowChanged() {
		auto* backing = mItem ? mItem->window() : nullptr;
		watchBacking(backing);
		setWindowInternal(backing ? backing->property("_qsWindow").value<QObject*>() : nullptr);
	}

	void setWindowInternal(QObject* window) {
		if (window == mWindow) return;
		QObject::disconnect(mWindowGone);
		mWindow = window;
		if (window) {
			mWindowGone = QObject::connect(window, &QObject::destroyed, this, [this] {
				mWindow = nullptr;
				emit windowChanged();
			});
		}
		emit windowChanged();
	}

	QPointer<QObject> mWindow;
	QMetaObject::Connection mWindowGone;
	QPointer<QQuickItem> mItem;
	QPointer<QQuickWindow> mBacking;
	QsPopupBox mRect;
	QsPopupMargins mMargins;
	int mEdges = Qt::TopEdge | Qt::LeftEdge;
	int mGravity = Qt::BottomEdge | Qt::RightEdge;
	int mAdjustment = 3; // PopupAdjustment.Slide
	QRect mWindowRect {0, 0, 1, 1};
};

// `QsWindow.window` / `QsWindow.contentItem` from any item: the Quickshell window object that
// owns the item's QQuickWindow (registered by the window via the "_qsWindow" property).
class QsWindowAttached: public QObject {
	Q_OBJECT;
	Q_PROPERTY(QObject* window READ window NOTIFY windowChanged);
	Q_PROPERTY(QQuickItem* contentItem READ contentItem NOTIFY windowChanged);
	QML_NAMED_ELEMENT(QsWindow);
	QML_UNCREATABLE("QsWindow is only available as an attached object.");
	QML_ATTACHED(QsWindowAttached);

public:
	explicit QsWindowAttached(QObject* parent): QObject(parent) {
		if (auto* item = qobject_cast<QQuickItem*>(parent)) {
			QObject::connect(item, &QQuickItem::windowChanged, this, [this](QQuickWindow* backing) {
				this->watchBacking(backing);
				emit this->windowChanged();
			});
			this->watchBacking(item->window());
		} else {
			this->watchBacking(qobject_cast<QQuickWindow*>(parent));
		}
	}

	[[nodiscard]] QObject* window() const {
		auto* item = qobject_cast<QQuickItem*>(this->parent());
		auto* win = item ? item->window() : qobject_cast<QQuickWindow*>(this->parent());
		return win ? win->property("_qsWindow").value<QObject*>() : nullptr;
	}

	[[nodiscard]] QQuickItem* contentItem() const {
		auto* item = qobject_cast<QQuickItem*>(this->parent());
		auto* win = item ? item->window() : nullptr;
		return win ? win->contentItem() : nullptr;
	}

	// Upstream's mapping helpers: coordinates in the Quickshell window's content item.
	Q_INVOKABLE [[nodiscard]] QPointF itemPosition(QQuickItem* item) const {
		auto* content = this->mappingRoot("itemPosition", item);
		return content ? content->mapFromItem(item, 0, 0) : QPointF();
	}
	Q_INVOKABLE [[nodiscard]] QRectF itemRect(QQuickItem* item) const {
		auto* content = this->mappingRoot("itemRect", item);
		return content ? content->mapFromItem(item, item->boundingRect()) : QRectF();
	}
	Q_INVOKABLE [[nodiscard]] QPointF mapFromItem(QQuickItem* item, QPointF point) const {
		auto* content = this->mappingRoot("mapFromItem", item);
		return content ? content->mapFromItem(item, point) : QPointF();
	}
	Q_INVOKABLE [[nodiscard]] QPointF mapFromItem(QQuickItem* item, qreal x, qreal y) const {
		return this->mapFromItem(item, QPointF(x, y));
	}
	Q_INVOKABLE [[nodiscard]] QRectF mapFromItem(QQuickItem* item, QRectF rect) const {
		auto* content = this->mappingRoot("mapFromItem", item);
		return content ? content->mapFromItem(item, rect) : QRectF();
	}
	Q_INVOKABLE [[nodiscard]] QRectF mapFromItem(QQuickItem* item, qreal x, qreal y, qreal width, qreal height) const {
		return this->mapFromItem(item, QRectF(x, y, width, height));
	}

	// Like upstream, an object that is not an item (a LazyLoader in a bar, say) answers for the
	// closest item among its parents.
	static QsWindowAttached* qmlAttachedProperties(QObject* object) {
		auto* owner = object;
		while (owner && !qobject_cast<QQuickItem*>(owner)) owner = owner->parent();
		return new QsWindowAttached(owner ? owner : object);
	}

signals:
	void windowChanged();

protected:
	// Content enters its QQuickWindow before the window registers (or, on reuse, swaps) its
	// Quickshell object: bindings read during creation would keep null without this cue.
	bool eventFilter(QObject* watched, QEvent* event) override {
		if (watched == mBacking && event->type() == QEvent::DynamicPropertyChange
		    && static_cast<QDynamicPropertyChangeEvent*>(event)->propertyName() == "_qsWindow")
		{
			emit windowChanged();
		}
		return false;
	}

private:
	void watchBacking(QQuickWindow* backing) {
		if (backing == mBacking) return;
		if (mBacking) mBacking->removeEventFilter(this);
		mBacking = backing;
		if (backing) backing->installEventFilter(this);
	}

	// The window's content item (ProxyWindowBase.contentItem), with upstream's complaints.
	[[nodiscard]] QQuickItem* mappingRoot(const char* call, QQuickItem* item) const {
		auto* window = this->window();
		if (!window) {
			qCritical().noquote() << "Cannot call" << call << "before item is a member of a window.";
			return nullptr;
		}
		if (!item) {
			qCritical() << "Cannot map position of null item.";
			return nullptr;
		}
		return window->property("contentItem").value<QQuickItem*>();
	}

	QPointer<QQuickWindow> mBacking;
};
