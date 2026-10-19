#pragma once
// DMA-BUF import through EGL for window capture (windowcast.rs, windowcast.h): which modifiers
// the driver imports as a 2D texture, and the import itself on the render thread's context.
#include <cstdint>
#include <vector>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <QtGui/QGuiApplication>
#include <QtGui/QOpenGLContext>
#include <QtGui/qopenglcontext_platform.h>
#include "rust/cxx.h"

namespace qsdmabuf {

inline constexpr std::uint64_t MOD_INVALID = 0x00ffffffffffffffULL;

/// Qt's EGLDisplay: the current context's on the render thread, else the one Mesa hands out for
/// Qt's wl_display (same native display, same EGLDisplay).
inline EGLDisplay display() {
	if (auto* ctx = QOpenGLContext::currentContext()) {
		if (auto* egl = ctx->nativeInterface<QNativeInterface::QEGLContext>()) return egl->display();
	}
	auto* app = qGuiApp ? qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>() : nullptr;
	if (!app) return EGL_NO_DISPLAY;
	static EGLDisplay dpy = [&] {
		auto d = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_KHR, app->display(), nullptr);
		if (d != EGL_NO_DISPLAY && !eglInitialize(d, nullptr, nullptr)) return EGL_NO_DISPLAY;
		return d;
	}();
	return dpy;
}

inline rust::Vec<std::uint64_t> dmabuf_modifiers(std::uint32_t fourcc) {
	rust::Vec<std::uint64_t> out;
	auto dpy = display();
	if (dpy == EGL_NO_DISPLAY) return out;
	auto query = reinterpret_cast<PFNEGLQUERYDMABUFMODIFIERSEXTPROC>(eglGetProcAddress("eglQueryDmaBufModifiersEXT"));
	EGLint n = 0;
	if (query && query(dpy, static_cast<EGLint>(fourcc), 0, nullptr, nullptr, &n) && n > 0) {
		std::vector<EGLuint64KHR> mods(n);
		std::vector<EGLBoolean> external(n);
		query(dpy, static_cast<EGLint>(fourcc), n, mods.data(), external.data(), &n);
		for (EGLint i = 0; i < n; i++) {
			// External-only layouts need samplerExternalOES; Qt's image node samples sampler2D.
			if (!external[i]) out.push_back(mods[i]);
		}
	}
	// Implicit layout (no modifier attributes), what the driver picks for a plain allocation.
	out.push_back(MOD_INVALID);
	return out;
}

struct Plane {
	int fd;
	std::uint32_t offset;
	std::uint32_t stride;
};

/// An EGLImage over the planes, or EGL_NO_IMAGE_KHR. EGL keeps its own reference to the buffer.
inline EGLImageKHR import(
    EGLDisplay dpy,
    std::uint32_t width,
    std::uint32_t height,
    std::uint32_t fourcc,
    std::uint64_t modifier,
    const Plane* planes,
    std::uint32_t count
) {
	static auto create = reinterpret_cast<PFNEGLCREATEIMAGEKHRPROC>(eglGetProcAddress("eglCreateImageKHR"));
	if (!create || count == 0 || count > 4) return EGL_NO_IMAGE_KHR;
	static const EGLint fdKey[] = {EGL_DMA_BUF_PLANE0_FD_EXT, EGL_DMA_BUF_PLANE1_FD_EXT, EGL_DMA_BUF_PLANE2_FD_EXT, EGL_DMA_BUF_PLANE3_FD_EXT};
	static const EGLint offsetKey[] = {EGL_DMA_BUF_PLANE0_OFFSET_EXT, EGL_DMA_BUF_PLANE1_OFFSET_EXT, EGL_DMA_BUF_PLANE2_OFFSET_EXT, EGL_DMA_BUF_PLANE3_OFFSET_EXT};
	static const EGLint pitchKey[] = {EGL_DMA_BUF_PLANE0_PITCH_EXT, EGL_DMA_BUF_PLANE1_PITCH_EXT, EGL_DMA_BUF_PLANE2_PITCH_EXT, EGL_DMA_BUF_PLANE3_PITCH_EXT};
	static const EGLint modLoKey[] = {EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE3_MODIFIER_LO_EXT};
	static const EGLint modHiKey[] = {EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT, EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT, EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT, EGL_DMA_BUF_PLANE3_MODIFIER_HI_EXT};
	std::vector<EGLint> attribs = {
	    EGL_WIDTH, static_cast<EGLint>(width), EGL_HEIGHT, static_cast<EGLint>(height),
	    EGL_LINUX_DRM_FOURCC_EXT, static_cast<EGLint>(fourcc),
	};
	for (std::uint32_t i = 0; i < count; i++) {
		attribs.insert(attribs.end(), {fdKey[i], planes[i].fd, offsetKey[i], static_cast<EGLint>(planes[i].offset), pitchKey[i], static_cast<EGLint>(planes[i].stride)});
		if (modifier != MOD_INVALID) {
			attribs.insert(attribs.end(), {modLoKey[i], static_cast<EGLint>(modifier & 0xffffffff), modHiKey[i], static_cast<EGLint>(modifier >> 32)});
		}
	}
	attribs.push_back(EGL_NONE);
	return create(dpy, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, nullptr, attribs.data());
}

inline void destroy(EGLDisplay dpy, EGLImageKHR image) {
	static auto destroyImage = reinterpret_cast<PFNEGLDESTROYIMAGEKHRPROC>(eglGetProcAddress("eglDestroyImageKHR"));
	if (destroyImage && image != EGL_NO_IMAGE_KHR) destroyImage(dpy, image);
}

} // namespace qsdmabuf
