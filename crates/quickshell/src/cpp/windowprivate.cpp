// The one place that needs Qt's private headers, so window.h and frameclock.h (included
// across crates) stay on the public API.
#include <QtGui/private/qwindow_p.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/private/qquickwindow_p.h>
#include <QtQuick/private/qsgadaptationlayer_p.h>
#include <QtQuick/private/qsgcontext_p.h>
#include <QtQuick/QSGRendererInterface>
#include <QtGui/QFont>
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
} // namespace qsrs
