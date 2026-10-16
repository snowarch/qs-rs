#pragma once
// ClippingRectangle.border: a QML value type, which only C++ can declare.
#include <QtCore/QObject>
#include <QtGui/QColor>
#include <QtQml/qqml.h>

class ClippingRectangleBorder {
	Q_GADGET;
	Q_PROPERTY(QColor color MEMBER color);
	Q_PROPERTY(bool pixelAligned MEMBER pixelAligned);
	Q_PROPERTY(int width MEMBER width);
	QML_VALUE_TYPE(clippingRectangleBorder);

public:
	QColor color = Qt::black;
	bool pixelAligned = true;
	int width = 0;

	bool operator==(const ClippingRectangleBorder& o) const {
		return color == o.color && pixelAligned == o.pixelAligned && width == o.width;
	}
};
