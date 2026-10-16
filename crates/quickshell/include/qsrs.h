#pragma once
// Small Qt helpers that cxx-qt-lib does not expose. Kept header-only and minimal.
#include <QtCore/QCryptographicHash>
#include <QtCore/QVariant>
#include <QtGui/QColor>
#include <QtGui/QIcon>
#include <QtGui/QImage>
#include <QtGui/QPainter>
#include <QtGui/QPixmap>
#include <QtQml/QQmlEngine>
#include <QtQuick/QQuickImageProvider>
#include "rust/cxx.h"

namespace qsrs {
inline QVariant null_variant() { return QVariant::fromValue(nullptr); }

inline QString qt_version() { return QString::fromLatin1(qVersion()); }

inline QString md5_hex(const QString& s) {
	return QString(QCryptographicHash::hash(s.toUtf8(), QCryptographicHash::Md5).toHex());
}

inline bool has_theme_icon(const QString& name) { return QIcon::hasThemeIcon(name); }

// image://icon/<name>[?path=<dir>|?fallback=<name>], same scheme as Quickshell.
class IconImageProvider: public QQuickImageProvider {
public:
	IconImageProvider(): QQuickImageProvider(QQuickImageProvider::Pixmap) {}

	QPixmap requestPixmap(const QString& id, QSize* size, const QSize& requestedSize) override {
		QString iconName = id, fallbackName, path;
		if (auto i = id.indexOf("?path="); i != -1) {
			iconName = id.sliced(0, i);
			path = QString("/%1/%2").arg(id.sliced(i + 6), iconName.sliced(iconName.lastIndexOf('/') + 1));
		} else if (auto j = id.indexOf("?fallback="); j != -1) {
			iconName = id.sliced(0, j);
			fallbackName = id.sliced(j + 10);
		}
		auto icon = QIcon::fromTheme(iconName);
		if (icon.isNull() && !fallbackName.isEmpty()) icon = QIcon::fromTheme(fallbackName);
		if (icon.isNull() && !path.isEmpty()) icon = QIcon(QPixmap(path));
		auto target = requestedSize.isValid() ? requestedSize : QSize(100, 100);
		if (target.width() == 0 || target.height() == 0) target = QSize(2, 2);
		auto pixmap = icon.pixmap(target.width(), target.height());
		if (pixmap.isNull()) {
			qWarning() << "Could not load icon" << id << "at size" << target << "from request";
			auto w = std::max(target.width() + target.width() % 2, 2);
			auto h = std::max(target.height() + target.height() % 2, 2);
			pixmap = QPixmap(w, h);
			pixmap.fill(QColorConstants::Black);
			QPainter p(&pixmap);
			p.fillRect(w / 2, 0, w / 2, h / 2, QColor(0xd900d8));
			p.fillRect(0, h / 2, w / 2, h / 2, QColor(0xd900d8));
		}
		if (size) *size = pixmap.size();
		return pixmap;
	}
};

inline void install_image_providers(QQmlEngine& engine) {
	engine.addImageProvider("icon", new IconImageProvider());
}
// ARGB pixels of an image file, optionally cropped and downscaled (keeping aspect) so neither side
// exceeds `rescale`. Reentrant: safe to call from worker threads.
inline rust::Vec<uint32_t> load_pixels(const QString& path, int32_t x, int32_t y, int32_t w, int32_t h, double rescale) {
	rust::Vec<uint32_t> out;
	auto image = QImage(path);
	if (image.isNull()) return out;
	if (w > 0 && h > 0) image = image.copy(x, y, w, h);
	if (rescale > 0 && (image.width() > rescale || image.height() > rescale)) {
		image = image.scaled(int(rescale), int(rescale), Qt::KeepAspectRatio, Qt::SmoothTransformation);
	}
	image = image.convertToFormat(QImage::Format_ARGB32);
	out.reserve(size_t(image.width()) * image.height());
	for (int iy = 0; iy < image.height(); iy++) {
		auto* line = reinterpret_cast<const QRgb*>(image.constScanLine(iy));
		for (int ix = 0; ix < image.width(); ix++) out.push_back(line[ix]);
	}
	return out;
}
} // namespace qsrs
