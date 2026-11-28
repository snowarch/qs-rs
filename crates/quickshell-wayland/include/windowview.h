#pragma once
// `WindowView` (qs-rs, rin-window-view-v1): another client's window, shown live inside this
// window by the compositor itself. Nothing is copied: the compositor draws the window's current
// content above this window's surface, scaled into the item's place, clipped to its rounded
// rect and to every clipping ancestor, at its effective opacity. Each request is double
// buffered and sent right before a frame, so a view moves with the frame that moves its item.
// The window goes beneath this window's surface, which gets a hole where it shows: what the item's
// siblings above it draw (a veil, an icon, a ring) stays over the window, as over any other item.
// Under an ancestor drawn through a texture (a layer, a ShaderEffectSource, a ClippingRectangle) the
// item's own hole clears only that texture: a second hole (WindowViewHole) goes right before each such
// ancestor, in its parent, so the window shows through every level and what the texture draws over the
// item (a veil, an icon) stays over the window. A ClippingRectangle's rounded box clips the view too.
#include <algorithm>
#include <array>
#include <cstring>
#include <vector>
#include <QtCore/QPointer>
#include <QtCore/QThread>
#include <QtGui/QWindow>
#include <QtQml/qqmlregistration.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/private/qquickitem_p.h>
#include <QtWaylandClient/private/qwaylanddisplay_p.h>
#include <QtWaylandClient/private/qwaylandintegration_p.h>
#include <QtWaylandClient/private/qwaylandwindow_p.h>
#include <QtGui/private/qhighdpiscaling_p.h>
#include "rin-window-view-v1-client-protocol.h"

namespace qswv {

// Bound once from Qt's registry; the compositor offers it to privileged clients only.
inline rin_window_view_manager_v1* manager() {
	static rin_window_view_manager_v1* manager = []() -> rin_window_view_manager_v1* {
		auto* integration = QtWaylandClient::QWaylandIntegration::instance();
		if (integration == nullptr) return nullptr;
		for (const auto& global: integration->display()->globals()) {
			if (global.interface != QLatin1String(rin_window_view_manager_v1_interface.name)) continue;
			return static_cast<rin_window_view_manager_v1*>(
			    wl_registry_bind(global.registry, global.id, &rin_window_view_manager_v1_interface, 1)
			);
		}
		return nullptr;
	}();
	return manager;
}

// What one view looks like in surface coordinates; a frame sends only what changed.
struct Look {
	QRectF destination;
	QRectF clip;
	std::array<qreal, 4> radii {}; // top left, top right, bottom right, bottom left
	qreal opacity = 1;
	bool beneath = true;
};

// The hole: clears the window's pixels by the rounded rect's coverage times the item's opacity.
class HoleMaterial: public QSGMaterial {
public:
	HoleMaterial() { this->setFlag(Blending); }
	[[nodiscard]] QSGMaterialType* type() const override {
		static QSGMaterialType type;
		return &type;
	}
	[[nodiscard]] QSGMaterialShader* createShader(QSGRendererInterface::RenderMode /*mode*/) const override;
	[[nodiscard]] int compare(const QSGMaterial* other) const override {
		const auto* o = static_cast<const HoleMaterial*>(other);
		if (this->radii != o->radii) return this->radii < o->radii ? -1 : 1;
		if (this->size.width() != o->size.width()) return this->size.width() < o->size.width() ? -1 : 1;
		if (this->size.height() != o->size.height()) return this->size.height() < o->size.height() ? -1 : 1;
		return 0;
	}
	std::array<float, 4> radii {}; // top left, top right, bottom right, bottom left
	QSizeF size;
};

class HoleShader: public QSGMaterialShader {
public:
	HoleShader() {
		this->setShaderFileName(VertexStage, QStringLiteral(":/qt/qml/Quickshell/Wayland/shaders/windowhole.vert.qsb"));
		this->setShaderFileName(FragmentStage, QStringLiteral(":/qt/qml/Quickshell/Wayland/shaders/windowhole.frag.qsb"));
		this->setFlag(UpdatesGraphicsPipelineState);
	}

	bool updateUniformData(RenderState& state, QSGMaterial* material, QSGMaterial* old) override {
		auto* data = state.uniformData()->data();
		bool changed = false;
		if (state.isMatrixDirty()) {
			std::memcpy(data, state.combinedMatrix().constData(), 64);
			changed = true;
		}
		if (state.isOpacityDirty()) {
			const float opacity = state.opacity();
			std::memcpy(data + 64, &opacity, 4);
			changed = true;
		}
		const auto* hole = static_cast<HoleMaterial*>(material);
		if (old == nullptr || hole->compare(old) != 0) {
			// std140: the vec4 starts at 80, the vec2 after it.
			std::memcpy(data + 80, hole->radii.data(), 16);
			const float size[2] = {float(hole->size.width()), float(hole->size.height())};
			std::memcpy(data + 96, size, 8);
			changed = true;
		}
		return changed;
	}

	bool updateGraphicsPipelineState(RenderState& /*state*/, GraphicsPipelineState* ps, QSGMaterial* /*material*/, QSGMaterial* /*old*/) override {
		ps->blendEnable = true;
		ps->srcColor = GraphicsPipelineState::Zero;
		ps->dstColor = GraphicsPipelineState::OneMinusSrcAlpha;
		ps->separateBlendFactors = true;
		ps->srcAlpha = GraphicsPipelineState::Zero;
		ps->dstAlpha = GraphicsPipelineState::OneMinusSrcAlpha;
		return true;
	}
};

inline QSGMaterialShader* HoleMaterial::createShader(QSGRendererInterface::RenderMode /*mode*/) const {
	return new HoleShader();
}

// The node that draws a hole of `rect` (item units) with per-corner radii; null when there is none.
inline QSGNode* holeNode(QSGNode* old, const QRectF& rect, const std::array<qreal, 4>& radii) {
	if (rect.isEmpty()) {
		delete old;
		return nullptr;
	}
	auto* node = static_cast<QSGGeometryNode*>(old);
	if (node == nullptr) {
		node = new QSGGeometryNode();
		auto* geometry = new QSGGeometry(QSGGeometry::defaultAttributes_TexturedPoint2D(), 4);
		geometry->setDrawingMode(QSGGeometry::DrawTriangleStrip);
		node->setGeometry(geometry);
		node->setMaterial(new HoleMaterial());
		node->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
	}
	QSGGeometry::updateTexturedRectGeometry(node->geometry(), rect, QRectF(0, 0, 1, 1));
	auto* material = static_cast<HoleMaterial*>(node->material());
	for (int i = 0; i < 4; i++) material->radii[i] = float(radii[i]);
	material->size = rect.size();
	node->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
	return node;
}

[[nodiscard]] inline bool near(const QPointF& a, const QPointF& b) {
	return qAbs(a.x() - b.x()) < 0.01 && qAbs(a.y() - b.y()) < 0.01;
}

// A rounded rect, in scene units: what a view shows in.
struct Shape {
	QRectF rect;
	std::array<qreal, 4> radii {};

	// Cut by another one: each corner keeps the radius of the shape that still owns it.
	void cut(const QRectF& other, const std::array<qreal, 4>& otherRadii) {
		const QRectF next = this->rect.intersected(other);
		const auto corners = [](const QRectF& r) {
			return std::array<QPointF, 4> {r.topLeft(), r.topRight(), r.bottomRight(), r.bottomLeft()};
		};
		const auto mine = corners(this->rect), theirs = corners(other), now = corners(next);
		for (int i = 0; i < 4; i++)
			this->radii[i] = qMax(near(now[i], mine[i]) ? this->radii[i] : 0, near(now[i], theirs[i]) ? otherRadii[i] : 0);
		this->rect = next;
	}
};

} // namespace qswv

// The hole a WindowView needs in the level a texture-drawn ancestor is drawn into: it sits right before that
// ancestor, so it clears what lies beneath the texture and nothing the texture itself draws. Zero-sized and
// transparent to positioners and layouts, so its parent's arrangement does not see it.
class WindowViewHole: public QQuickItem {
public:
	explicit WindowViewHole(QQuickItem* parent): QQuickItem(parent) {
		this->setFlag(ItemHasContents);
		QQuickItemPrivate::get(this)->setTransparentForPositioner(true);
	}

	void place(const QRectF& rect, const std::array<qreal, 4>& radii) {
		if (rect == this->rect && radii == this->radii) return;
		this->rect = rect;
		this->radii = radii;
		this->update();
	}

protected:
	QSGNode* updatePaintNode(QSGNode* old, UpdatePaintNodeData* /*data*/) override {
		return qswv::holeNode(old, this->rect, this->radii);
	}

private:
	QRectF rect;
	std::array<qreal, 4> radii {};
};

namespace qswv {

} // namespace qswv

// Whether the compositor shows windows inside the shell's surfaces (ToplevelManager.canViewWindows).
class QsWindowViewSupport: public QObject {
	Q_OBJECT;
	Q_PROPERTY(bool available READ available CONSTANT);
	QML_ELEMENT;
	QML_SINGLETON;

public:
	[[nodiscard]] static bool available() { return qswv::manager() != nullptr; }
};

class WindowView: public QQuickItem {
	Q_OBJECT;
	/// The window's ext-foreign-toplevel identifier. Empty shows nothing.
	Q_PROPERTY(QString toplevelId READ toplevelId WRITE setToplevelId NOTIFY toplevelIdChanged);
	/// The window's logical size, for its aspect under PreserveAspectFit and PreserveAspectCrop.
	/// Without one the window fills the item.
	Q_PROPERTY(QSizeF sourceSize READ sourceSize WRITE setSourceSize NOTIFY sourceSizeChanged);
	/// Image.Stretch, Image.PreserveAspectFit or Image.PreserveAspectCrop (the default).
	Q_PROPERTY(int fillMode READ fillMode WRITE setFillMode NOTIFY fillModeChanged);
	/// The corner radius of the item's rect, which the view stays inside.
	Q_PROPERTY(qreal radius READ radius WRITE setRadius NOTIFY radiusChanged);
	/// Whether the compositor is asked to show the window now.
	Q_PROPERTY(bool showing READ showing NOTIFY showingChanged);
	QML_ELEMENT;

public:
	enum { Stretch = 0, PreserveAspectFit = 1, PreserveAspectCrop = 2 };

	explicit WindowView(QQuickItem* parent = nullptr): QQuickItem(parent) {
		this->setFlag(ItemHasContents);
	}
	~WindowView() override { this->drop(); }

	[[nodiscard]] QString toplevelId() const { return this->mToplevelId; }
	void setToplevelId(const QString& id) {
		if (id == this->mToplevelId) return;
		this->mToplevelId = id;
		this->frame();
		emit this->toplevelIdChanged();
	}

	[[nodiscard]] QSizeF sourceSize() const { return this->mSourceSize; }
	void setSourceSize(const QSizeF& size) {
		if (size == this->mSourceSize) return;
		this->mSourceSize = size;
		this->frame();
		emit this->sourceSizeChanged();
	}

	[[nodiscard]] int fillMode() const { return this->mFillMode; }
	void setFillMode(int mode) {
		if (mode == this->mFillMode) return;
		this->mFillMode = mode;
		this->frame();
		emit this->fillModeChanged();
	}

	[[nodiscard]] qreal radius() const { return this->mRadius; }
	void setRadius(qreal radius) {
		// A negative radius is a protocol error on the compositor side, fatal to the whole connection.
		radius = qMax<qreal>(0, radius);
		if (qFuzzyCompare(radius, this->mRadius)) return;
		this->mRadius = radius;
		this->frame();
		emit this->radiusChanged();
	}

	[[nodiscard]] bool showing() const { return this->view != nullptr; }

signals:
	void toplevelIdChanged();
	void sourceSizeChanged();
	void fillModeChanged();
	void radiusChanged();
	void showingChanged();

protected:
	void itemChange(ItemChange change, const ItemChangeData& data) override {
		if (change == ItemSceneChange) this->attach(data.window);
		else if (change == ItemVisibleHasChanged || change == ItemOpacityHasChanged) this->frame();
		QQuickItem::itemChange(change, data);
	}

	void componentComplete() override {
		QQuickItem::componentComplete();
		this->attach(this->window());
	}

private:
	void frame() {
		if (this->mWindow) this->mWindow->update();
	}

	void attach(QQuickWindow* window) {
		if (window == this->mWindow) return;
		this->drop();
		if (this->mWindow) QObject::disconnect(this->mWindow, nullptr, this, nullptr);
		this->mWindow = window;
		this->syncsHere = false;
		if (window == nullptr) return;
		QObject::connect(window, &QQuickWindow::afterAnimating, this, &WindowView::onBeforeFrame);
		// Where the scene graph syncs on this thread (qs-rs's basic loop) the look is sent there, after every
		// afterAnimating handler has moved what it moves this frame.
		QObject::connect(window, &QQuickWindow::beforeSynchronizing, this, &WindowView::onSync, Qt::DirectConnection);
		this->frame();
	}

	void onBeforeFrame() {
		if (!this->syncsHere) this->send();
	}

	void onSync() {
		if (QThread::currentThread() != this->thread()) return;
		this->syncsHere = true;
		this->send();
	}

	// The view goes with the next commit; a view whose surface went away is inert, and goes too.
	void drop() {
		if (this->view == nullptr) return;
		rin_window_view_v1_destroy(this->view);
		this->view = nullptr;
		this->wlSurface = nullptr;
		this->sent = {};
		this->mHole = QRectF();
		this->holdLevels({}, {});
		this->update();
		this->frame();
		emit this->showingChanged();
	}

	[[nodiscard]] qreal effectiveOpacity() const {
		qreal opacity = 1;
		for (const QQuickItem* item = this; item != nullptr && opacity > 0; item = item->parentItem())
			opacity *= item->opacity();
		return opacity;
	}

	// The opacity the window is drawn at, so that it shows at the item's opacity e. The hole clears the surface by
	// e, so what the shell draws beneath the item keeps (1 - e) of its alpha over the window; drawn at e the window
	// would let the desktop through by (1 - e) twice over. What lies beneath is taken as opaque and faded by the
	// opacity the whole window shares (its window-wide ancestors, a fade in or out), so the shell leaves
	// 1 - (1 - e) * shared of the window in sight, and e over that shows it at e: 1 while the window stands.
	[[nodiscard]] qreal drawnOpacity(qreal e) const {
		if (e >= 1 || this->mWindow == nullptr) return e;
		const QSizeF size = this->mWindow->size();
		qreal shared = 1;
		for (const QQuickItem* item = this->parentItem(); item != nullptr; item = item->parentItem())
			if (item->width() >= size.width() - 1 && item->height() >= size.height() - 1) shared *= item->opacity();
		const qreal left = 1 - (1 - e) * shared;
		return left > 0 ? qBound(0.0, e / left, 1.0) : 1;
	}

	// The items, this one and its ancestors, drawn through a texture (a layer or a ShaderEffectSource takes them),
	// innermost first.
	[[nodiscard]] std::vector<QQuickItem*> textured() const {
		std::vector<QQuickItem*> out;
		for (QQuickItem* item = const_cast<WindowView*>(this); item != nullptr; item = item->parentItem()) {
			auto* d = QQuickItemPrivate::get(item);
			if (d->extra.isAllocated() && d->extra->effectRefCount > 0) out.push_back(item);
		}
		return out;
	}

	// A ClippingRectangle (Quickshell.Widgets) clips by its rounded box in a shader, not by `clip`.
	[[nodiscard]] static bool roundedClip(const QQuickItem* item, std::array<qreal, 4>& radii) {
		const auto* meta = item->metaObject();
		if (meta->indexOfProperty("contentInsideBorder") < 0 || meta->indexOfProperty("bottomRightRadius") < 0) return false;
		radii = {
		    item->property("topLeftRadius").toReal(),
		    item->property("topRightRadius").toReal(),
		    item->property("bottomRightRadius").toReal(),
		    item->property("bottomLeftRadius").toReal(),
		};
		return true;
	}

	// Each texture-drawn level gets its hole, right before it in its parent, shaped as what the view shows and as
	// opaque as the items between the level and the view are.
	void holdLevels(const std::vector<QQuickItem*>& levels, const qswv::Shape& shape) {
		std::vector<std::pair<QPointer<QQuickItem>, QPointer<WindowViewHole>>> next;
		for (auto* level: levels) {
			auto* parent = level->parentItem();
			WindowViewHole* hole = nullptr;
			for (auto& held: this->levels) {
				if (held.first != level || held.second == nullptr || held.second->parentItem() != parent) continue;
				hole = held.second;
				held.second = nullptr;
				break;
			}
			if (hole == nullptr) hole = new WindowViewHole(parent);
			hole->setZ(level->z());
			hole->stackBefore(level);
			hole->setPosition(level->position());
			qreal opacity = 1;
			for (QQuickItem* item = this; item != nullptr; item = item->parentItem()) {
				opacity *= item->opacity();
				if (item == level) break;
			}
			hole->setOpacity(opacity);
			const QRectF rect = hole->mapRectFromScene(shape.rect);
			const qreal k = shape.rect.width() > 0 ? rect.width() / shape.rect.width() : 1;
			hole->place(rect, {shape.radii[0] * k, shape.radii[1] * k, shape.radii[2] * k, shape.radii[3] * k});
			next.emplace_back(level, hole);
		}
		for (auto& held: this->levels) {
			if (held.second == nullptr) continue;
			held.second->setParentItem(nullptr);
			held.second->deleteLater();
		}
		this->levels = std::move(next);
	}

	// Where the window goes and what it stays inside, in window coordinates, the hole in item coordinates and
	// the texture-drawn levels it goes through; false when nothing shows.
	bool look(qswv::Look& out, QRectF& hole, qreal& holeRadius, std::vector<QQuickItem*>& levels) const {
		if (!this->isVisible() || this->width() <= 0 || this->height() <= 0) return false;
		out.opacity = qBound(0.0, this->effectiveOpacity(), 1.0);
		if (out.opacity <= 0) return false;
		const QRectF box(0, 0, this->width(), this->height());
		QRectF destination = box;
		const auto& source = this->mSourceSize;
		if (this->mFillMode != Stretch && source.width() > 0 && source.height() > 0) {
			const qreal aspect = source.width() / source.height();
			const bool wider = box.width() / box.height() > aspect;
			const bool byHeight = (this->mFillMode == PreserveAspectFit) == wider;
			const QSizeF size = byHeight ? QSizeF(box.height() * aspect, box.height()) : QSizeF(box.width(), box.width() / aspect);
			destination = QRectF(box.center().x() - size.width() / 2, box.center().y() - size.height() / 2, size.width(), size.height());
		}
		// The rounded rect the window shows in: the item's (the destination's, when it fits inside), cut by every
		// clipping ancestor and every ClippingRectangle. A corner keeps the radius of the shape that owns it.
		hole = this->mFillMode == PreserveAspectFit ? destination : box;
		holeRadius = qMax<qreal>(0, qMin(this->mRadius, qMin(hole.width(), hole.height()) / 2));
		out.destination = this->mapRectToScene(destination);
		const QRectF own = this->mapRectToScene(hole);
		if (own.isEmpty()) return false;
		const qreal radius = holeRadius * own.width() / hole.width();
		qswv::Shape shape {own, {radius, radius, radius, radius}};
		for (const QQuickItem* item = this->parentItem(); item != nullptr && !shape.rect.isEmpty(); item = item->parentItem()) {
			std::array<qreal, 4> radii {};
			if (roundedClip(item, radii)) {
				const QRectF box = item->mapRectToScene(QRectF(0, 0, item->width(), item->height()));
				const qreal k = item->width() > 0 ? box.width() / item->width() : 1;
				shape.cut(box, {radii[0] * k, radii[1] * k, radii[2] * k, radii[3] * k});
			} else if (item->clip()) {
				shape.cut(item->mapRectToScene(item->clipRect()), {});
			}
		}
		if (shape.rect.isEmpty()) return false;
		out.clip = shape.rect;
		out.radii = shape.radii;
		levels = this->textured();
		// A level without a parent has nowhere to put its hole: the window goes above everything then.
		out.beneath = std::all_of(levels.begin(), levels.end(), [](QQuickItem* level) { return level->parentItem() != nullptr; });
		if (out.beneath) out.opacity = this->drawnOpacity(out.opacity);
		return true;
	}

	void send() {
		auto* ww = this->mWindow ? dynamic_cast<QtWaylandClient::QWaylandWindow*>(this->mWindow->handle()) : nullptr;
		auto* surface = ww ? ww->wlSurface() : nullptr;
		if (ww != this->waylandWindow) {
			if (this->waylandWindow) QObject::disconnect(this->waylandWindow, nullptr, this, nullptr);
			this->waylandWindow = ww;
			if (ww) QObject::connect(ww, &QtWaylandClient::QWaylandWindow::surfaceDestroyed, this, &WindowView::drop);
		}
		qswv::Look look;
		QRectF hole;
		qreal holeRadius = 0;
		std::vector<QQuickItem*> levels;
		auto* manager = qswv::manager();
		if (manager == nullptr || surface == nullptr || this->mToplevelId.isEmpty() || !this->look(look, hole, holeRadius, levels)) {
			this->drop();
			return;
		}
		const qswv::Shape shape {look.clip, look.radii};
		if (this->view != nullptr && (this->wlSurface != surface || this->viewOf != this->mToplevelId)) this->drop();
		// Window coordinates to surface coordinates: Qt's own scaling, then the frame's margins.
		const qreal scale = QHighDpiScaling::factor(static_cast<QWindow*>(this->mWindow.data()));
		const auto margins = ww->clientSideMargins();
		const auto place = [&](const QRectF& r) {
			return QRectF(r.x() * scale + margins.left(), r.y() * scale + margins.top(), r.width() * scale, r.height() * scale);
		};
		look.destination = place(look.destination);
		look.clip = place(look.clip);
		// An ancestor's corner can come in negative; the compositor refuses one.
		for (auto& radius: look.radii) radius = qMax<qreal>(0, radius) * scale;
		const bool fresh = this->view == nullptr;
		if (fresh) {
			this->view = rin_window_view_manager_v1_get_window_view(manager, surface, this->mToplevelId.toUtf8().constData());
			this->wlSurface = surface;
			this->viewOf = this->mToplevelId;
		}
		if (fresh || look.destination != this->sent.destination) {
			const auto& d = look.destination;
			rin_window_view_v1_set_destination(this->view, wl_fixed_from_double(d.x()), wl_fixed_from_double(d.y()), wl_fixed_from_double(d.width()), wl_fixed_from_double(d.height()));
		}
		if (fresh || look.clip != this->sent.clip || look.radii != this->sent.radii) {
			const auto& c = look.clip;
			const auto& r = look.radii;
			rin_window_view_v1_set_clip(this->view, wl_fixed_from_double(c.x()), wl_fixed_from_double(c.y()), wl_fixed_from_double(c.width()), wl_fixed_from_double(c.height()),
			    wl_fixed_from_double(r[0]), wl_fixed_from_double(r[1]), wl_fixed_from_double(r[2]), wl_fixed_from_double(r[3]));
		}
		if (fresh || !qFuzzyCompare(look.opacity, this->sent.opacity)) rin_window_view_v1_set_opacity(this->view, wl_fixed_from_double(look.opacity));
		if (fresh || look.beneath != this->sent.beneath) rin_window_view_v1_set_order(this->view, look.beneath ? -1 : 0);
		this->sent = look;
		this->holdLevels(look.beneath ? levels : std::vector<QQuickItem*> {}, shape);
		// The hole goes with this same frame: the sync that follows updates this item's node.
		if (!look.beneath) hole = QRectF();
		if (hole != this->mHole || !qFuzzyCompare(1 + holeRadius, 1 + this->mHoleRadius)) {
			this->mHole = hole;
			this->mHoleRadius = holeRadius;
			this->update();
		}
		if (fresh) emit this->showingChanged();
	}

protected:
	QSGNode* updatePaintNode(QSGNode* old, UpdatePaintNodeData* /*data*/) override {
		const qreal r = this->mHoleRadius;
		return qswv::holeNode(old, this->view == nullptr ? QRectF() : this->mHole, {r, r, r, r});
	}

private:
	QString mToplevelId;
	QSizeF mSourceSize;
	int mFillMode = PreserveAspectCrop;
	qreal mRadius = 0;
	QPointer<QQuickWindow> mWindow;
	QPointer<QtWaylandClient::QWaylandWindow> waylandWindow;
	rin_window_view_v1* view = nullptr;
	wl_surface* wlSurface = nullptr;
	QString viewOf;
	qswv::Look sent;
	QRectF mHole;
	qreal mHoleRadius = 0;
	std::vector<std::pair<QPointer<QQuickItem>, QPointer<WindowViewHole>>> levels;
	bool syncsHere = false;
};
