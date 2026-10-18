#pragma once

#include <QtCore/QDebug>
#include <QtCore/QEvent>
#include <QtCore/QHash>
#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtCore/QRegularExpression>
#include <QtCore/QTimer>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>

// Window reuse across reloads (upstream ProxyWindowBase::onReload / disownWindow). Before the old
// generation goes, every shown window that can be reused is frozen (no frame is rendered, so the
// compositor keeps showing its last one), loses its old content and outlives its proxy here; the
// new generation's proxies take them back by key (declaring file, subtype tag, screen), so a
// reload never unmaps a bar or drops its exclusive zone. Windows nobody claims go afterwards.
class QsWindowFreeze: public QObject {
public:
	bool eventFilter(QObject* watched, QEvent* event) override {
		Q_UNUSED(watched);
		return event->type() == QEvent::Expose || event->type() == QEvent::UpdateRequest;
	}
	static QsWindowFreeze* instance() {
		static auto* filter = new QsWindowFreeze();
		return filter;
	}
};

struct QsWindowPool {
	static QList<QPointer<QQuickWindow>>& live() {
		static QList<QPointer<QQuickWindow>> windows;
		return windows;
	}
	static QHash<QString, QList<QPointer<QQuickWindow>>>& stashed() {
		static QHash<QString, QList<QPointer<QQuickWindow>>> pool;
		return pool;
	}

	// Called right before the old root is deleted.
	static void stash() {
		auto& live = QsWindowPool::live();
		live.removeAll(nullptr);
		for (auto& window: live) {
			auto key = window->property("_qsReuseKey").toString();
			if (key.isEmpty() || !window->isVisible() || window->property("_qsStashed").toBool()) continue;
			window->installEventFilter(QsWindowFreeze::instance());
			window->setProperty("_qsFrozen", true);
			for (auto* child: window->contentItem()->childItems()) child->setParentItem(nullptr);
			static_cast<QObject*>(window.data())->setParent(nullptr);
			window->setProperty("_qsStashed", true);
			window->setProperty("_qsWindow", QVariant());
			stashed()[key].append(window);
		}
	}

	// Where a proxy sits in its generation's tree, like upstream's positional matching: per
	// ancestor, its type and index among same-type siblings (QML type names carry a per-engine
	// suffix, dropped here).
	static QString treePath(QObject* object) {
		static const QRegularExpression suffix("(_QML(TYPE)?_\\d+)+$");
		QStringList parts;
		for (auto* o = object; o != nullptr && o->parent() != nullptr; o = o->parent()) {
			int index = 0;
			for (auto* sibling: o->parent()->children()) {
				if (sibling == o) break;
				if (sibling->metaObject() == o->metaObject()) index++;
			}
			parts.prepend(QString(o->metaObject()->className()).remove(suffix) + '#' + QString::number(index));
		}
		return parts.join('/');
	}

	static QQuickWindow* take(const QString& key) {
		auto it = stashed().find(key);
		while (it != stashed().end() && !it->isEmpty()) {
			auto window = it->takeFirst();
			if (window) {
				window->setProperty("_qsStashed", false);
				reused()++;
				return window;
			}
		}
		return nullptr;
	}

	// Called once the new root is loaded. Shells create windows after that too (inir's deferred
	// loaders, Loaders, incubation), so what nobody claimed goes a while later, unless another
	// reload stashed meanwhile.
	static void drop() {
		auto serial = ++dropSerial();
		QTimer::singleShot(3000, [serial] {
			if (serial == dropSerial()) dropNow();
		});
	}

	static int& dropSerial() {
		static int serial = 0;
		return serial;
	}

	static void dropNow() {
		int closed = 0;
		for (auto& list: stashed()) {
			for (auto& window: list) {
				if (window) {
					window->deleteLater();
					closed++;
				}
			}
		}
		stashed().clear();
		if (reused() + closed > 0) qInfo().noquote() << QString("Windows kept across the reload: %1 (closed: %2)").arg(reused()).arg(closed);
		reused() = 0;
	}

	static int& reused() {
		static int count = 0;
		return count;
	}
};
