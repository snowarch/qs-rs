#pragma once
// ScreencopyView.sourceRect for both drawing halves (screencopy.h, windowcast.h).
#include <algorithm>
#include <utility>
#include <QtCore/QRectF>
#include <QtGui/QTransform>

namespace qscrop {

/// ScreencopyView.sourceRect, picked on a frame `base` big, on a frame `size` big. Each edge keeps
/// its distance to the nearer side, so a sidebar or a status line stays put when the window
/// resizes and a span across the middle grows with it. Empty shows the whole frame.
inline QRectF map(const QRectF& rect, QSizeF base, const QSizeF& size) {
	if (size.isEmpty()) return {};
	if (rect.isEmpty()) return {QPointF(), size};
	if (base.isEmpty()) base = size;
	auto axis = [](qreal a, qreal b, qreal from, qreal to) {
		auto edge = [&](qreal e) { return e * 2 <= from ? e : to - (from - e); };
		a = std::clamp(edge(a), 0.0, to);
		b = std::clamp(edge(b), 0.0, to);
		if (b - a < 1) {
			b = std::min(to, a + 1);
			a = std::max(0.0, b - 1);
		}
		return std::pair {a, b};
	};
	auto [x0, x1] = axis(rect.left(), rect.right(), base.width(), size.width());
	auto [y0, y1] = axis(rect.top(), rect.bottom(), base.height(), size.height());
	return {QPointF(x0, y0), QPointF(x1, y1)};
}

/// A rect of the transformed view (what sourceSize describes) in the buffer's own pixels, undoing
/// what QsScreencopyItem draws: rotate by 90 * (transform & 3) around the center, then mirror.
inline QRectF toBuffer(const QRectF& view, const QSizeF& buffer, int transform) {
	auto swap = (transform & 1) != 0;
	auto vw = swap ? buffer.height() : buffer.width(), vh = swap ? buffer.width() : buffer.height();
	QTransform m;
	m.translate(buffer.width() / 2, buffer.height() / 2);
	m.rotate(-90.0 * (transform & 3));
	if (transform & 4) m.scale(-1, 1);
	m.translate(-vw / 2, -vh / 2);
	return m.mapRect(view);
}

} // namespace qscrop
