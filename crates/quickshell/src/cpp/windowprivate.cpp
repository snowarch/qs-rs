// The one place that needs Qt's private headers, so window.h and frameclock.h (included
// across crates) stay on the public API.
#include <QtGui/private/qwindow_p.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/private/qquickwindow_p.h>
#include <QtQuick/private/qquickitem_p.h>
#include <QtQuick/private/qquicktext_p.h>
#include <QtQml/private/qqmldata_p.h>
#include <QtQml/private/qqmlcontextdata_p.h>
#include <QtQuick/private/qsgadaptationlayer_p.h>
#include <QtQuick/private/qsgcontext_p.h>
#include <QtQuick/QSGRendererInterface>
#include <QtGui/QFont>
#include <algorithm>
#include "quickshell/include/frameclock.h"

namespace qsrs {
bool setWindowUpdatesEnabled(QQuickWindow* window, bool enabled) {
	auto* d = QQuickWindowPrivate::get(window);
	if (d->updatesEnabled == enabled) return false;
	d->updatesEnabled = enabled;
	return true;
}

namespace {
// Read access to the two members the trim needs: m_glyphCaches is protected (reached through a
// subclass member pointer), m_registeredNodes private (reached through an explicit
// instantiation, the one place access checks do not apply).
struct GlyphCaches: QSGRenderContext {
	static constexpr auto caches = &GlyphCaches::m_glyphCaches;
};
struct RegisteredNodes {
	using type = QSGDistanceFieldGlyphConsumerList QSGDistanceFieldGlyphCache::*;
	friend type member(RegisteredNodes);
};
template <typename Tag, typename Tag::type M>
struct Expose {
	friend typename Tag::type member(Tag) { return M; }
};
template struct Expose<RegisteredNodes, &QSGDistanceFieldGlyphCache::m_registeredNodes>;
} // namespace

// A distance-field glyph cache lives in its window's render context for as long as the context
// does (Qt deletes the idle ones only when the font database changes, by relaying out every
// text and rebuilding every cache). Its reference font holds the font engine, the engine holds
// the FreeType face, and the face keeps the font file mapped: a Design's fonts stayed after its
// last text was gone. This deletes just the caches no glyph node uses or points at (what
// QSGDefaultRenderContext::invalidateGlyphCaches does for its idle ones), frees the native
// text engines nothing renders any more and drops the render thread's font cache. Runs on the
// render thread before the next sync, the stage where Qt touches them itself.
void releaseUnusedGlyphCaches(QQuickWindow* window) {
	if (!window->isVisible() || !window->isSceneGraphInitialized()) return;
	if (window->rendererInterface()->graphicsApi() == QSGRendererInterface::Software) return;
	window->scheduleRenderJob(QRunnable::create([context = QQuickWindowPrivate::get(window)->context] {
		auto& caches = context->*GlyphCaches::caches;
		for (auto it = caches.begin(); it != caches.end();) {
			if ((*it)->isActive() || !((*it)->*member(RegisteredNodes())).isEmpty()) {
				++it;
			} else {
				delete *it;
				it = caches.erase(it);
			}
		}
		context->flushGlyphCaches();
		QFont::cleanup();
	}), QQuickWindow::BeforeSynchronizingStage);
	window->update();
}

bool frameclock::update_pending(QWindow* window) { return QWindowPrivate::get(window)->updateRequestPending; }

bool frameclock::scene_waits(QWindow* window) {
	auto* qw = qobject_cast<QQuickWindow*>(window);
	if (!qw) return false;
	auto* d = QQuickWindowPrivate::get(qw);
	return d->updatesEnabled && d->isRenderable() && (d->dirtyItemList || !d->itemsToPolish.isEmpty());
}
QString frameclock::dirty_items(QWindow* window, int max) {
	auto* qw = qobject_cast<QQuickWindow*>(window);
	if (!qw) return {};
	QHash<QString, int> seen;
	QHash<QString, quint32> dirt;
	QStringList order;
	for (auto* item = QQuickWindowPrivate::get(qw)->dirtyItemList; item; item = QQuickItemPrivate::get(item)->nextDirtyItem) {
		auto key = QString::fromLatin1(item->metaObject()->className());
		if (auto cut = key.indexOf(QLatin1String("_QML")); cut > 0) key.truncate(cut);
		if (auto* data = QQmlData::get(item); data && data->outerContext)
			key += QStringLiteral("@%1:%2").arg(data->outerContext->url().fileName()).arg(data->lineNumber);
		if (!seen.contains(key)) order << key;
		seen[key]++;
		dirt[key] |= QQuickItemPrivate::get(item)->dirtyAttributes;
	}
	std::stable_sort(order.begin(), order.end(), [&](const QString& a, const QString& b) { return seen[a] > seen[b]; });
	// What it costs: c content rebuilt (text laid out, geometry made), s size, t moved or turned, o opacity.
	auto flags = [](quint32 d) {
		QString f;
		if (d & QQuickItemPrivate::Content) f += QLatin1Char('c');
		if (d & QQuickItemPrivate::Size) f += QLatin1Char('s');
		if (d & (QQuickItemPrivate::TransformOrigin | QQuickItemPrivate::Transform | QQuickItemPrivate::BasicTransform | QQuickItemPrivate::Position)) f += QLatin1Char('t');
		if (d & QQuickItemPrivate::OpacityValue) f += QLatin1Char('o');
		return f.isEmpty() ? f : QLatin1Char(':') + f;
	};
	QStringList out;
	for (const auto& key: order.mid(0, max))
		out << (seen[key] > 1 ? QStringLiteral("%1%2*%3").arg(key, flags(dirt[key])).arg(seen[key]) : key + flags(dirt[key]));
	if (order.size() > max) out << QStringLiteral("+%1").arg(order.size() - max);
	return out.join(QLatin1Char('|'));
}
// Qt 6.12: QQuickText::init sets ItemObservesViewport "until size is known" and only setText
// recomputes it (past 10000 characters), returning early when the text doesn't change. A Text
// born empty (an icon-only label, a rolling number's spare digit) keeps it for good, and
// QQuickTextPrivate::transformChanged then rebuilds its node for every move or scale of any
// ancestor: ~120 per frame while the iRiS island opened its Control Center.
void frameclock::quiet_short_texts(QWindow* window) {
	auto* qw = qobject_cast<QQuickWindow*>(window);
	if (!qw) return;
	for (auto* item = QQuickWindowPrivate::get(qw)->dirtyItemList; item; item = QQuickItemPrivate::get(item)->nextDirtyItem) {
		if (!(item->flags() & QQuickItem::ItemObservesViewport)) continue;
		if (!(QQuickItemPrivate::get(item)->dirtyAttributes & QQuickItemPrivate::Content)) continue;
		if (auto* text = qobject_cast<QQuickText*>(item); text && text->text().size() <= 10000)
			text->setFlag(QQuickItem::ItemObservesViewport, false);
	}
}

} // namespace qsrs
