#pragma once
// Clipboard text for Quickshell.clipboardText.
#include <QtCore/QObject>
#include <QtGui/QClipboard>
#include <QtGui/QGuiApplication>
#include <QtQml/qqml.h>

class QsClipboard: public QObject {
	Q_OBJECT;
	Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged);
	QML_ELEMENT;
	QML_SINGLETON;

public:
	explicit QsClipboard(QObject* parent = nullptr): QObject(parent) {
		QObject::connect(QGuiApplication::clipboard(), &QClipboard::changed, this, [this](QClipboard::Mode mode) {
			if (mode == QClipboard::Clipboard) emit this->textChanged();
		});
	}
	QString text() const { return QGuiApplication::clipboard()->text(); }
	void setText(const QString& text) { QGuiApplication::clipboard()->setText(text); }

signals:
	void textChanged();
};
