#pragma once
// ScreencopyView's drawing half for windows: takes the newest DMA-BUF frame from the Rust capture
// (windowcast.rs) on the render thread, imports it once per buffer as an EGLImage-backed GL
// texture and shows it like QsScreencopyItem (aspect kept, centered). The buffer on screen stays
// with the item until a newer one replaces it; then it goes back to the compositor.
#include <cstdint>
#include <unordered_map>
#include <unistd.h>
#include <QtCore/QObject>
#include <QtCore/QRunnable>
#include <QtGui/QOpenGLContext>
#include <QtGui/QOpenGLFunctions>
#include <QtQml/qqmlregistration.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGImageNode>
#include <QtQuick/qsgtexture_platform.h>
#include "quickshell_wayland/include/dmabuf.h"
#include "quickshell_wayland/include/crop.h"
#include "quickshell_wayland/src/castffi.cxx.h"

namespace qswindowcast {

struct Imported {
	EGLImageKHR image = EGL_NO_IMAGE_KHR;
	GLuint texture = 0;
	QSGTexture* sg = nullptr;
};

using Cache = std::unordered_map<std::uint64_t, Imported>;

/// Frees imported buffers; GL objects only on the render thread with its context current.
inline void freeImported(Imported& imported) {
	delete imported.sg;
	if (auto* ctx = QOpenGLContext::currentContext()) {
		if (imported.texture) ctx->functions()->glDeleteTextures(1, &imported.texture);
		qsdmabuf::destroy(qsdmabuf::display(), imported.image);
	}
	imported = {};
}

class CleanupJob: public QRunnable {
public:
	CleanupJob(Cache cache, std::uint64_t frames, std::uint64_t held, bool hasHeld)
	    : mCache(std::move(cache)), mFrames(frames), mHeld(held), mHasHeld(hasHeld) {}
	~CleanupJob() override { finish(); }
	void run() override { finish(); }

private:
	void finish() {
		for (auto& [key, imported]: this->mCache) freeImported(imported);
		this->mCache.clear();
		if (this->mFrames) {
			if (this->mHasHeld) cast_release(this->mFrames, this->mHeld);
			cast_unref(this->mFrames);
			this->mFrames = 0;
		}
	}
	Cache mCache;
	std::uint64_t mFrames;
	std::uint64_t mHeld;
	bool mHasHeld;
};

} // namespace qswindowcast

class QsWindowCastItem: public QQuickItem {
	Q_OBJECT;
	/// QsWindowCapture.frames: where frames come from.
	Q_PROPERTY(quint64 frames READ frames WRITE setFrames NOTIFY framesChanged);
	/// ScreencopyView.sourceRect and the frame size it was picked on, mapped per frame (qscrop::map)
	/// so a resize never shows one frame cropped for the old size.
	Q_PROPERTY(QRectF sourceRect READ sourceRect WRITE setSourceRect NOTIFY sourceRectChanged);
	Q_PROPERTY(QSizeF sourceRectBase READ sourceRectBase WRITE setSourceRectBase NOTIFY sourceRectBaseChanged);
	QML_NAMED_ELEMENT(QsWindowCastItem);

public:
	explicit QsWindowCastItem(QQuickItem* parent = nullptr): QQuickItem(parent) {
		this->setFlag(QQuickItem::ItemHasContents);
	}
	~QsWindowCastItem() override {
		this->dropResources();
		if (this->mFrames) cast_unref(this->mFrames);
	}

	[[nodiscard]] quint64 frames() const { return this->mFrames; }
	void setFrames(quint64 frames) {
		if (frames == this->mFrames) return;
		this->dropResources();
		if (this->mFrames) cast_unref(this->mFrames);
		this->mFrames = frames;
		if (frames) cast_retain(frames);
		this->update();
		emit this->framesChanged();
	}

	[[nodiscard]] QRectF sourceRect() const { return this->mSourceRect; }
	void setSourceRect(const QRectF& rect) {
		if (rect == this->mSourceRect) return;
		this->mSourceRect = rect;
		this->update();
		emit this->sourceRectChanged();
	}
	[[nodiscard]] QSizeF sourceRectBase() const { return this->mSourceRectBase; }
	void setSourceRectBase(const QSizeF& base) {
		if (base == this->mSourceRectBase) return;
		this->mSourceRectBase = base;
		this->update();
		emit this->sourceRectBaseChanged();
	}

	Q_INVOKABLE static QRectF mapCrop(const QRectF& rect, const QSizeF& base, const QSizeF& size) {
		return qscrop::map(rect, base, size);
	}

signals:
	void framesChanged();
	void sourceRectChanged();
	void sourceRectBaseChanged();

protected:
	void releaseResources() override { this->dropResources(); }

	void itemChange(ItemChange change, const ItemChangeData& data) override {
		if (change == ItemSceneChange && data.window == nullptr) this->dropResources();
		QQuickItem::itemChange(change, data);
	}

	QSGNode* updatePaintNode(QSGNode* oldNode, UpdatePaintNodeData* /*unused*/) override {
		if (!this->mFrames || !this->window() || !QOpenGLContext::currentContext()) {
			delete oldNode;
			return nullptr;
		}
		if (!this->mHasRef) {
			cast_retain(this->mFrames);
			this->mHasRef = true;
		}
		this->takeFrame();
		this->purge();

		auto current = this->mCache.find(this->mCurrent);
		if (!this->mHasCurrent || current == this->mCache.end()) {
			delete oldNode;
			return nullptr;
		}
		auto* node = static_cast<QSGImageNode*>(oldNode);
		if (!node) {
			node = this->window()->createImageNode();
			node->setOwnsTexture(false);
			node->setFiltering(QSGTexture::Linear);
		}
		node->setTexture(current->second.sg);
		// Only the sampled texels change: same fragments, same frame, nothing copied.
		auto source = qscrop::map(this->mSourceRect, this->mSourceRectBase, QSizeF(this->mSize));
		node->setSourceRect(source);

		auto rect = this->boundingRect();
		auto size = source.size();
		size.scale(rect.width(), rect.height(), Qt::KeepAspectRatio);
		node->setRect(
		    rect.x() + rect.width() / 2 - size.width() / 2,
		    rect.y() + rect.height() / 2 - size.height() / 2,
		    size.width(),
		    size.height()
		);
		return node;
	}

private:
	void takeFrame() {
		CastFrame frame {};
		if (!cast_take(this->mFrames, frame)) return;
		auto it = this->mCache.find(frame.key);
		if (it == this->mCache.end()) {
			auto imported = this->import(frame);
			if (imported.sg) it = this->mCache.emplace(frame.key, imported).first;
		}
		for (std::uint32_t i = 0; i < frame.planes; i++) {
			if (frame.fds[i] >= 0) ::close(frame.fds[i]);
		}
		if (it == this->mCache.end()) {
			cast_release(this->mFrames, frame.key);
			return;
		}
		// The one shown until now was drawn in the frame already submitted: back to niri.
		if (this->mHasCurrent && this->mCurrent != frame.key) cast_release(this->mFrames, this->mCurrent);
		this->mCurrent = frame.key;
		this->mHasCurrent = true;
		this->mSize = QSize(static_cast<int>(frame.width), static_cast<int>(frame.height));
	}

	qswindowcast::Imported import(const CastFrame& frame) {
		qswindowcast::Imported out;
		qsdmabuf::Plane planes[4];
		for (std::uint32_t i = 0; i < frame.planes && i < 4; i++) planes[i] = {frame.fds[i], frame.offsets[i], frame.strides[i]};
		auto dpy = qsdmabuf::display();
		out.image = qsdmabuf::import(dpy, frame.width, frame.height, frame.fourcc, frame.modifier, planes, frame.planes);
		if (out.image == EGL_NO_IMAGE_KHR) {
			if (!this->mWarned) qWarning() << "Window capture: EGL refused a DMA-BUF, error" << Qt::hex << eglGetError();
			this->mWarned = true;
			return {};
		}
		using TargetTexture = void (*)(GLenum, void*);
		static auto target = reinterpret_cast<TargetTexture>(eglGetProcAddress("glEGLImageTargetTexture2DOES"));
		auto* gl = QOpenGLContext::currentContext()->functions();
		gl->glGenTextures(1, &out.texture);
		gl->glBindTexture(GL_TEXTURE_2D, out.texture);
		gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
		gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
		gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
		gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
		if (target) target(GL_TEXTURE_2D, out.image);
		gl->glBindTexture(GL_TEXTURE_2D, 0);
		// XRGB reads alpha as 1; ARGB windows (translucent terminals) are premultiplied by niri.
		auto options = frame.fourcc == static_cast<std::uint32_t>(DRM_FORMAT_ARGB) ? QQuickWindow::TextureHasAlphaChannel : QQuickWindow::CreateTextureOptions();
		out.sg = QNativeInterface::QSGOpenGLTexture::fromNative(
		    out.texture,
		    this->window(),
		    QSize(static_cast<int>(frame.width), static_cast<int>(frame.height)),
		    options
		);
		if (!target || !out.sg) qswindowcast::freeImported(out);
		return out;
	}

	/// Buffers the stream dropped (a resize replaces all of them) leave the cache.
	void purge() {
		for (auto it = this->mCache.begin(); it != this->mCache.end();) {
			if ((!this->mHasCurrent || it->first != this->mCurrent) && !cast_alive(this->mFrames, it->first)) {
				qswindowcast::freeImported(it->second);
				it = this->mCache.erase(it);
			} else {
				++it;
			}
		}
	}

	/// Hands the cache to the render thread to free, and the held buffer and the render thread's
	/// reference back (the property keeps its own until the item dies or the frames change).
	void dropResources() {
		if (!this->mHasRef && this->mCache.empty()) return;
		auto* job = new qswindowcast::CleanupJob(std::move(this->mCache), this->mHasRef ? this->mFrames : 0, this->mCurrent, this->mHasCurrent);
		this->mCache = {};
		this->mHasRef = false;
		this->mHasCurrent = false;
		if (auto* w = this->window()) w->scheduleRenderJob(job, QQuickWindow::NoStage);
		else delete job;
	}

	static constexpr std::uint32_t DRM_FORMAT_ARGB = 0x34325241; // 'AR24'
	quint64 mFrames = 0;
	bool mHasRef = false;
	qswindowcast::Cache mCache;
	std::uint64_t mCurrent = 0;
	bool mHasCurrent = false;
	QSize mSize;
	QRectF mSourceRect;
	QSizeF mSourceRectBase;
	bool mWarned = false;
};
