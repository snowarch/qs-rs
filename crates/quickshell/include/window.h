#pragma once
// Window glue that needs QML's attached-property machinery or QWindow API that cxx-qt cannot
// express: QsWindow attached lookup and input masks.
#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtGui/QRegion>
#include <QtGui/QWindow>
#include <QtQml/QQmlContext>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQml/QQmlListProperty>
#include <cmath>

namespace ExclusionMode {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Normal = 0, Ignore = 1, Auto = 2 };
Q_ENUM_NS(Enum);
} // namespace ExclusionMode

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

	[[nodiscard]] QRegion build() const {
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

	Q_INVOKABLE void registerProxy(QQuickWindow* window, QObject* proxy) {
		if (window) window->setProperty("_qsWindow", QVariant::fromValue(proxy));
	}

	// `url` resolved against the file that instantiated `object` (upstream's
	// qmlContext(this)->resolvedUrl for relative paths like LazyLoader.source).
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
			QObject::connect(item, &QQuickItem::windowChanged, this, &QsWindowAttached::windowChanged);
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

	static QsWindowAttached* qmlAttachedProperties(QObject* object) { return new QsWindowAttached(object); }

signals:
	void windowChanged();
};
