#pragma once
// `WlrLayershell` attached properties for PanelWindow (layer, namespace, keyboard focus), plus the
// WlrLayer / WlrKeyboardFocus enums. PanelWindow.qml reads these and drives LayerShellQt.
#include <QtCore/QObject>
#include <QtCore/QVariant>
#include <QtQml/qqml.h>

namespace WlrLayer {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Background = 0, Bottom = 1, Top = 2, Overlay = 3 };
Q_ENUM_NS(Enum);
} // namespace WlrLayer

namespace WlrKeyboardFocus {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { None = 0, Exclusive = 1, OnDemand = 2 };
Q_ENUM_NS(Enum);
} // namespace WlrKeyboardFocus

class WlrLayershell: public QObject {
	Q_OBJECT;
	Q_PROPERTY(WlrLayer::Enum layer READ layer WRITE setLayer NOTIFY layerChanged);
	Q_PROPERTY(QString namespace READ ns WRITE setNamespace NOTIFY namespaceChanged);
	Q_PROPERTY(WlrKeyboardFocus::Enum keyboardFocus READ keyboardFocus WRITE setKeyboardFocus NOTIFY keyboardFocusChanged);
	// Forwarded to the panel itself, as in quickshell.
	Q_PROPERTY(QVariant exclusionMode READ exclusionMode WRITE setExclusionMode NOTIFY exclusionModeChanged);
	Q_PROPERTY(QVariant exclusiveZone READ exclusiveZone WRITE setExclusiveZone NOTIFY exclusiveZoneChanged);
	QML_ELEMENT;
	QML_UNCREATABLE("WlrLayershell is used as an attached object on PanelWindow.");
	QML_ATTACHED(WlrLayershell);

public:
	explicit WlrLayershell(QObject* parent = nullptr): QObject(parent) {}

	WlrLayer::Enum layer() const { return this->mLayer; }
	void setLayer(WlrLayer::Enum layer) {
		if (layer == this->mLayer) return;
		this->mLayer = layer;
		emit this->layerChanged();
	}

	QString ns() const { return this->mNamespace; }
	void setNamespace(const QString& ns) {
		if (ns == this->mNamespace) return;
		this->mNamespace = ns;
		emit this->namespaceChanged();
	}

	WlrKeyboardFocus::Enum keyboardFocus() const { return this->mFocus; }
	void setKeyboardFocus(WlrKeyboardFocus::Enum focus) {
		if (focus == this->mFocus) return;
		this->mFocus = focus;
		emit this->keyboardFocusChanged();
	}

	QVariant exclusionMode() const { return this->parent() ? this->parent()->property("exclusionMode") : QVariant(); }
	void setExclusionMode(const QVariant& v) {
		if (this->parent()) this->parent()->setProperty("exclusionMode", v);
		emit this->exclusionModeChanged();
	}
	QVariant exclusiveZone() const { return this->parent() ? this->parent()->property("exclusiveZone") : QVariant(); }
	void setExclusiveZone(const QVariant& v) {
		if (this->parent()) this->parent()->setProperty("exclusiveZone", v);
		emit this->exclusiveZoneChanged();
	}

	static WlrLayershell* qmlAttachedProperties(QObject* object) { return new WlrLayershell(object); }

signals:
	void layerChanged();
	void namespaceChanged();
	void keyboardFocusChanged();
	void exclusionModeChanged();
	void exclusiveZoneChanged();

private:
	WlrLayer::Enum mLayer = WlrLayer::Top;
	QString mNamespace = "quickshell";
	WlrKeyboardFocus::Enum mFocus = WlrKeyboardFocus::None;
};

#include <LayerShellQt/Window>
#include <QtGui/QWindow>

// Applies PanelWindow's layer surface configuration to its backing window through LayerShellQt.
class QsLayerShell: public QObject {
	Q_OBJECT;
	QML_ELEMENT;
	QML_SINGLETON;

public:
	Q_INVOKABLE void configure(QWindow* window, int anchors, int layer, int keyboard, int exclusiveZone, int left, int top, int right, int bottom, const QString& scope) {
		if (!window) return;
		auto* ls = LayerShellQt::Window::get(window);
		ls->setAnchors(LayerShellQt::Window::Anchors(anchors));
		ls->setLayer(LayerShellQt::Window::Layer(layer));
		ls->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivity(keyboard));
		ls->setExclusiveZone(exclusiveZone);
		ls->setMargins(QMargins(left, top, right, bottom));
		ls->setScope(scope);
		ls->setActivateOnShow(keyboard != 0);
	}
};
