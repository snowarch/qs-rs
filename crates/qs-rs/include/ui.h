#pragma once
// `qs ui ...`: what a person would read on screen, so a test can point at things by their words instead of pixels.
// Every visible item of every shown window, in screen pixels (a layer surface placed from its anchors and margins),
// minus what is hidden, see-through or clipped away by an ancestor.
//
//   ui                  every visible text, window by window
//   ui windows          the shown windows and where they sit
//   ui find WORDS       items whose text or objectName holds WORDS (any case)
//   ui at X Y           what is under a point in the window that takes the pointer there, bottom to top
//   ui textures         images sharing one decoded picture (same url, size, cache) but asking it with and without
//                       mipmaps: Qt keeps one texture per picture, so the later one flips it ("QSGPlainTexture:
//                       Mipmap settings changed without having image data available")
// `in:NAME` before any of them keeps one window (its layer namespace or the file declaring it, a prefix).
//
// One line per item: `cx cy  x y w h  window  Type  "text"  flags`, a text renderer's rect being its ink; flags: t (draws
// text), hover, pressed, focus, off (disabled), cut (text elided), over (text runs past its box), part (an ancestor
// cuts its side off), edge (only its top or bottom, as a scrolled list does), glyph (an icon font). Ink of no colour
// (a field's hidden placeholder) is not listed.
#include <QtCore/QHash>
#include <QtCore/QMap>
#include <QtCore/QSet>
#include <QtCore/QStringList>
#include <QtGui/QColor>
#include <QtGui/QFont>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <algorithm>

namespace qsrs::ui {

inline QString windowName(QQuickWindow* w) {
	auto ns = w->property("_qsNamespace").toString();
	if (!ns.isEmpty()) return ns;
	auto* proxy = w->property("_qsWindow").value<QObject*>();
	auto* ctx = qmlContext(proxy ? proxy : w);
	auto out = ctx ? ctx->baseUrl().fileName() : (w->title().isEmpty() ? QString(w->metaObject()->className()) : w->title());
	return out.replace(' ', '_');
}

// Wayland never tells a client where its surface is: a layer surface is placed here as the compositor places it
// (anchors + margins on its output); any other window answers its own position, which niri leaves at 0,0.
inline QPointF windowOrigin(QQuickWindow* w, bool* known = nullptr) {
	auto anchors = w->property("_qsAnchors");
	auto* screen = w->screen();
	if (known) *known = anchors.isValid();
	if (!anchors.isValid() || !screen) return w->position();
	int a = anchors.toInt();
	auto m = w->property("_qsMargins").toList();
	int ml = m.value(0).toInt(), mt = m.value(1).toInt(), mr = m.value(2).toInt(), mb = m.value(3).toInt();
	auto g = screen->geometry();
	bool top = a & 1, bottom = a & 2, left = a & 4, right = a & 8;
	qreal x = left ? ml : right ? g.width() - w->width() - mr : (g.width() - w->width()) / 2.0;
	qreal y = top ? mt : bottom ? g.height() - w->height() - mb : (g.height() - w->height()) / 2.0;
	return QPointF(g.x() + x, g.y() + y);
}

struct Hit {
	QQuickItem* item;
	QRectF rect; // screen, clipped
	bool part; // a side cut off
	bool edge; // only its top or bottom
};

inline QString typeName(QObject* o) {
	QString n = o->metaObject()->className();
	// QML types show as Foo_QMLTYPE_12; Qt's own as QQuickFoo.
	if (auto i = n.indexOf("_QML"); i > 0) n.truncate(i);
	if (n.startsWith("QQuick")) n = n.mid(6);
	return n;
}

inline bool drawsText(QQuickItem* item) {
	return item->inherits("QQuickText") || item->inherits("QQuickTextInput") || item->inherits("QQuickTextEdit");
}

inline QString textOf(QQuickItem* item) {
	auto v = item->property("text");
	return v.typeId() == QMetaType::QString ? v.toString() : QString();
}

inline QString flagsOf(QQuickItem* item, bool part, bool edge = false) {
	QStringList f;
	auto prop = [item](const char* name) { return item->property(name); };
	if (prop("containsMouse").toBool() || prop("hovered").toBool()) f << "hover";
	if (drawsText(item)) f << "t";
	if (prop("pressed").toBool()) f << "pressed";
	if (item->hasActiveFocus()) f << "focus";
	if (!item->isEnabled()) f << "off";
	if (prop("truncated").toBool()) f << "cut";
	auto cw = prop("contentWidth");
	// A text input scrolls its line: running past the box is how it works.
	if (cw.isValid() && !textOf(item).isEmpty() && prop("elide").toInt() == 0 && prop("wrapMode").toInt() == 0
	    && cw.toReal() > item->width() + 1.5 && !item->inherits("QQuickTextInput") && !item->inherits("QQuickTextEdit"))
		f << "over";
	if (part) f << "part";
	else if (edge) f << "edge";
	auto font = prop("font");
	if (font.isValid() && font.value<QFont>().family().contains("Symbols", Qt::CaseInsensitive)) f << "glyph";
	return f.join(',');
}

// Paint order: children by z, ties in declaration order.
inline QList<QQuickItem*> paintOrder(QQuickItem* item) {
	auto kids = item->childItems();
	std::stable_sort(kids.begin(), kids.end(), [](QQuickItem* a, QQuickItem* b) { return a->z() < b->z(); });
	return kids;
}

// A text renderer is measured by the ink it lays down, not its box: a left-aligned word in a wide box overlaps
// nothing past its last letter.
inline QRectF inkRect(QQuickItem* item) {
	QRectF box(0, 0, item->width(), item->height());
	if (!drawsText(item)) return box;
	qreal cw = std::min(item->property("contentWidth").toReal(), box.width());
	qreal ch = std::min(item->property("contentHeight").toReal(), box.height());
	if (cw <= 0 || ch <= 0) return box;
	int h = item->property("effectiveHorizontalAlignment").isValid() ? item->property("effectiveHorizontalAlignment").toInt()
	                                                              : item->property("horizontalAlignment").toInt();
	int v = item->property("verticalAlignment").toInt();
	qreal x = (h & Qt::AlignRight) ? box.width() - cw : (h & Qt::AlignHCenter) ? (box.width() - cw) / 2 : 0;
	qreal y = (v & Qt::AlignBottom) ? box.height() - ch : (v & Qt::AlignVCenter) ? (box.height() - ch) / 2 : 0;
	return QRectF(x, y, cw, ch);
}

inline bool inkless(QQuickItem* item) {
	if (!drawsText(item)) return false;
	auto c = item->property("color");
	return c.isValid() && c.value<QColor>().alphaF() < 0.02;
}

// Items drawn through a texture (a ShaderEffectSource's source: ClippingRectangle, masks) end at its edge.
// By the visual tree: a window's items are not all QObject children of its content item.
inline void textured(QQuickItem* item, QSet<QQuickItem*>& out) {
	if (item->inherits("QQuickShaderEffectSource"))
		if (auto* source = item->property("sourceItem").value<QQuickItem*>()) out.insert(source);
	for (auto* kid: item->childItems()) textured(kid, out);
}

inline void walk(QQuickItem* item, const QPointF& origin, QRectF clip, qreal opacity, QList<Hit>& out, const QSet<QQuickItem*>& cut) {
	if (!item->isVisible()) return;
	opacity *= item->opacity();
	if (opacity < 0.02) return;
	auto scene = item->mapRectToScene(inkRect(item));
	auto screen = scene.translated(origin);
	auto seen = screen.intersected(clip);
	if (inkless(item)) {
	} else if (item->width() > 0 && item->height() > 0) {
		bool side = seen.width() < screen.width() - 0.5, ends = seen.height() < screen.height() - 0.5;
		if (!seen.isEmpty()) out << Hit { item, seen, side, ends && !side };
	} else {
		// Zero-sized holders still parent visible things.
		out << Hit { item, QRectF(screen.topLeft(), QSizeF(0, 0)), false, false };
	}
	// A layer draws the item into a texture of its own size: what lies outside is cut like a clip.
	auto* layer = item->property("layer").value<QObject*>();
	if (item->clip() || (layer && layer->property("enabled").toBool()) || cut.contains(item)) {
		clip = clip.intersected(item->mapRectToScene(QRectF(0, 0, item->width(), item->height())).translated(origin));
		if (clip.isEmpty()) return;
	}
	for (auto* kid: paintOrder(item)) walk(kid, origin, clip, opacity, out, cut);
}

inline QString line(const Hit& h, const QString& win) {
	auto r = h.rect;
	auto text = textOf(h.item).replace('\n', "\\n");
	if (text.size() > 80) text = text.left(79) + "…";
	auto name = h.item->objectName();
	return QString("%1 %2  %3 %4 %5 %6  %7  %8%9  \"%10\"  %11")
	    .arg(qRound(r.center().x()))
	    .arg(qRound(r.center().y()))
	    .arg(qRound(r.x()))
	    .arg(qRound(r.y()))
	    .arg(qRound(r.width()))
	    .arg(qRound(r.height()))
	    .arg(win, typeName(h.item), name.isEmpty() ? QString() : "#" + name, text, flagsOf(h.item, h.part, h.edge))
	    .trimmed();
}

// Every image item under `item`, shown or not, keyed by the picture Qt decodes once for all of them.
inline void images(QQuickItem* item, const QString& win, QMap<QString, QStringList>& users, QHash<QString, QSet<bool>>& mipmaps) {
	if (item->inherits("QQuickImageBase") && item->property("cache").toBool()) {
		const auto url = item->property("source").toUrl();
		if (!url.isEmpty()) {
			const auto size = item->property("sourceSize").toSize();
			const bool mipmap = item->property("mipmap").toBool();
			const auto key = QString("%1 @ %2x%3").arg(url.toString()).arg(size.width()).arg(size.height());
			QString parent = item->parentItem() ? typeName(item->parentItem()) : QString();
			users[key] << QString("  %1  %2 in %3  mipmap %4%5").arg(win, typeName(item), parent).arg(mipmap ? "on" : "off")
			                  .arg(item->isVisible() ? "" : "  hidden");
			mipmaps[key].insert(mipmap);
		}
	}
	for (auto* child: item->childItems()) images(child, win, users, mipmaps);
}

inline QString report(const QString& arg) {
	auto words = arg.split(' ', Qt::SkipEmptyParts);
	QString only;
	if (!words.isEmpty() && words.first().startsWith("in:")) only = words.takeFirst().mid(3);
	QString mode = words.isEmpty() ? QString() : words.takeFirst();
	QStringList out;
	QPointF point(words.value(0).toDouble(), words.value(1).toDouble());
	// Topmost layer first, so `at` can stop at the window that takes the pointer there.
	QList<QQuickWindow*> windows;
	for (auto* qw: QGuiApplication::topLevelWindows())
		if (auto* w = qobject_cast<QQuickWindow*>(qw); w && w->isVisible() && w->contentItem()) windows << w;
	std::stable_sort(windows.begin(), windows.end(), [](QQuickWindow* a, QQuickWindow* b) {
		return a->property("_qsLayer").toInt() > b->property("_qsLayer").toInt();
	});
	if (mode == "textures") {
		QMap<QString, QStringList> users;
		QHash<QString, QSet<bool>> mipmaps;
		for (auto* w: windows) {
			auto name = windowName(w);
			if (only.isEmpty() || name.startsWith(only, Qt::CaseInsensitive)) images(w->contentItem(), name, users, mipmaps);
		}
		for (auto it = users.cbegin(); it != users.cend(); ++it)
			if (mipmaps.value(it.key()).size() > 1) out << it.key() << it.value();
		return out.isEmpty() ? QString("no picture is asked with and without mipmaps") : out.join('\n');
	}
	for (auto* w: windows) {
		auto name = windowName(w);
		if (!only.isEmpty() && !name.startsWith(only, Qt::CaseInsensitive)) continue;
		bool known = false;
		auto origin = windowOrigin(w, &known);
		QRectF bounds(origin, QSizeF(w->width(), w->height()));
		if (mode == "at") {
			if (!bounds.contains(point)) continue;
			// An input mask that leaves the point out lets the pointer through to the window below.
			auto mask = w->mask();
			if (!mask.isEmpty() && !mask.contains((point - origin).toPoint())) continue;
		}
		if (mode == "windows") {
			out << QString("%1  %2 %3 %4 %5%6").arg(name).arg(qRound(origin.x())).arg(qRound(origin.y())).arg(w->width()).arg(w->height())
			           .arg(known ? "" : "  (position unknown to the client)");
			continue;
		}
		QList<Hit> hits;
		QSet<QQuickItem*> cut;
		textured(w->contentItem(), cut);
		walk(w->contentItem(), origin, bounds, 1.0, hits, cut);
		if (mode == "at") {
			// Only what has a name to it: text, an objectName, a state, or a type of the shell's own.
			for (auto& h: hits) {
				if (!h.rect.contains(point)) continue;
				QString cls = h.item->metaObject()->className();
				if (textOf(h.item).isEmpty() && h.item->objectName().isEmpty() && flagsOf(h.item, false).isEmpty()
				    && !cls.contains("_QML") && !cls.contains("MouseArea") && !cls.contains("Flickable"))
					continue;
				out << line(h, name);
			}
			break;
		}
		auto needle = words.join(' ');
		for (auto& h: hits) {
			if (h.rect.isEmpty()) continue;
			auto text = textOf(h.item);
			if (mode == "find") {
				if (!text.contains(needle, Qt::CaseInsensitive) && !h.item->objectName().contains(needle, Qt::CaseInsensitive)) continue;
			} else if (text.trimmed().isEmpty()) {
				continue;
			}
			out << line(h, name);
		}
	}
	if (mode != "" && mode != "windows" && mode != "find" && mode != "at")
		return "unknown ui action (windows, find WORDS, at X Y, textures; in:WINDOW first keeps one window)";
	return out.isEmpty() ? QString("nothing") : out.join('\n');
}

} // namespace qsrs::ui
