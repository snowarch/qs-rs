#pragma once
// ScreencopyView's drawing half: shows the QImage frames the Rust capture (screencopy.rs) hands
// over, placed like upstream's WlBufferQSGDisplayNode (buffer transform applied around the
// item's center, aspect kept, centered). A source rect only changes which texels get sampled.
#include <QtCore/QObject>
#include <QtGui/QImage>
#include <QtGui/QMatrix4x4>
#include <QtQml/qqmlregistration.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGImageNode>
#include <QtQuick/QSGTransformNode>
#include "quickshell_wayland/include/crop.h"

class QsScreencopyItem: public QQuickItem {
	Q_OBJECT;
	Q_PROPERTY(QImage image READ image WRITE setImage NOTIFY imageChanged);
	/// wl_output transform of the buffer (upstream's WlBufferTransform).
	Q_PROPERTY(int bufferTransform READ bufferTransform WRITE setBufferTransform NOTIFY bufferTransformChanged);
	/// The part shown, in transformed (sourceSize) pixels; empty shows all of it.
	Q_PROPERTY(QRectF sourceRect READ sourceRect WRITE setSourceRect NOTIFY sourceRectChanged);
	QML_NAMED_ELEMENT(QsScreencopyItem);

public:
	explicit QsScreencopyItem(QQuickItem* parent = nullptr): QQuickItem(parent) {
		this->setFlag(QQuickItem::ItemHasContents);
	}

	[[nodiscard]] QImage image() const { return this->mImage; }
	void setImage(const QImage& image) {
		this->mImage = image;
		this->mNewImage = true;
		this->update();
		emit this->imageChanged();
	}

	[[nodiscard]] int bufferTransform() const { return this->mTransform; }
	void setBufferTransform(int transform) {
		if (transform == this->mTransform) return;
		this->mTransform = transform;
		this->update();
		emit this->bufferTransformChanged();
	}

	[[nodiscard]] QRectF sourceRect() const { return this->mSourceRect; }
	void setSourceRect(const QRectF& rect) {
		if (rect == this->mSourceRect) return;
		this->mSourceRect = rect;
		this->update();
		emit this->sourceRectChanged();
	}

	Q_INVOKABLE static QRectF bufferRect(const QRectF& view, const QSizeF& buffer, int transform) {
		return qscrop::toBuffer(view, buffer, transform);
	}

signals:
	void imageChanged();
	void bufferTransformChanged();
	void sourceRectChanged();

protected:
	QSGNode* updatePaintNode(QSGNode* oldNode, UpdatePaintNodeData* /*unused*/) override {
		if (this->mImage.isNull() || !this->window()) {
			delete oldNode;
			return nullptr;
		}

		auto* node = static_cast<QSGTransformNode*>(oldNode);
		QSGImageNode* imageNode = nullptr;
		if (!node) {
			node = new QSGTransformNode();
			imageNode = this->window()->createImageNode();
			imageNode->setOwnsTexture(true);
			node->appendChildNode(imageNode);
			this->mNewImage = true;
		} else {
			imageNode = static_cast<QSGImageNode*>(node->firstChild());
		}

		if (this->mNewImage) {
			// The texture keeps the QImage only until it is uploaded, which hands the shared
			// memory back to the capture.
			imageNode->setTexture(this->window()->createTextureFromImage(this->mImage));
			this->mNewImage = false;
		}

		auto rect = this->boundingRect();
		auto flip = (this->mTransform & 4) != 0;
		auto degrees = static_cast<float>(90 * (this->mTransform & 3));
		auto matrix = QMatrix4x4();
		auto center = rect.center();
		matrix.translate(static_cast<float>(center.x()), static_cast<float>(center.y()));
		matrix.rotate(flip ? 180 : 0, 0, 1, 0);
		matrix.rotate(degrees, 0, 0, 1);
		matrix.translate(static_cast<float>(-center.x()), static_cast<float>(-center.y()));

		auto viewRect = matrix.mapRect(rect);
		auto source = QRectF(QPointF(), QSizeF(this->mImage.size()));
		if (!this->mSourceRect.isEmpty()) source = qscrop::toBuffer(this->mSourceRect, source.size(), this->mTransform) & source;
		imageNode->setSourceRect(source);
		auto bufferSize = source.size();
		bufferSize.scale(viewRect.width(), viewRect.height(), Qt::KeepAspectRatio);
		imageNode->setRect(
		    viewRect.x() + viewRect.width() / 2 - bufferSize.width() / 2,
		    viewRect.y() + viewRect.height() / 2 - bufferSize.height() / 2,
		    bufferSize.width(),
		    bufferSize.height()
		);
		imageNode->setFiltering(QSGTexture::Linear);
		node->setMatrix(matrix);
		return node;
	}

private:
	QImage mImage;
	bool mNewImage = false;
	int mTransform = 0;
	QRectF mSourceRect;
};
