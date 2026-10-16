#pragma once
// ObjectModel (upstream core/model.hpp): a list model of QObjects with a `values` list, usable both
// as `model:` (role modelData) and as a JS array. Assigning `values` applies upstream's
// diffUpdate (removals, then inserts/moves in order) so delegates of kept objects survive.
// Upstream's is uncreatable and read-only; qs-rs service singletons create and fill it from QML.
#include <QtCore/QAbstractListModel>
#include <QtQml/qqml.h>

class QsObjectModel: public QAbstractListModel {
	Q_OBJECT;
	Q_PROPERTY(QList<QObject*> values READ values WRITE setValues NOTIFY valuesChanged);
	QML_NAMED_ELEMENT(ObjectModel);

public:
	explicit QsObjectModel(QObject* parent = nullptr): QAbstractListModel(parent) {}

	[[nodiscard]] QHash<int, QByteArray> roleNames() const override { return {{Qt::UserRole, "modelData"}}; }

	[[nodiscard]] int rowCount(const QModelIndex& parent = QModelIndex()) const override {
		return parent.isValid() ? 0 : static_cast<int>(this->mValues.length());
	}

	[[nodiscard]] QVariant data(const QModelIndex& index, int role) const override {
		if (role != Qt::UserRole || index.row() < 0 || index.row() >= this->mValues.length()) return {};
		return QVariant::fromValue(this->mValues.at(index.row()));
	}

	[[nodiscard]] QList<QObject*> values() const { return this->mValues; }

	void setValues(const QList<QObject*>& next) {
		for (qsizetype i = 0; i < this->mValues.length();) {
			if (next.contains(this->mValues.at(i))) i++;
			else this->removeAt(i);
		}
		qsizetype oi = 0;
		for (auto* object: next) {
			if (object == nullptr) continue;
			if (this->mValues.length() == oi || this->mValues.at(oi) != object) {
				auto old = this->mValues.indexOf(object, oi);
				if (old != -1) this->removeAt(old);
				this->insertAt(object, oi);
			}
			oi++;
		}
	}

	Q_INVOKABLE qsizetype indexOf(QObject* object) const { return this->mValues.indexOf(object); }

signals:
	void valuesChanged();
	void objectInsertedPre(QObject* object, qsizetype index);
	void objectInsertedPost(QObject* object, qsizetype index);
	void objectRemovedPre(QObject* object, qsizetype index);
	void objectRemovedPost(QObject* object, qsizetype index);

private:
	void insertAt(QObject* object, qsizetype index) {
		emit this->objectInsertedPre(object, index);
		this->beginInsertRows(QModelIndex(), static_cast<int>(index), static_cast<int>(index));
		this->mValues.insert(index, object);
		this->endInsertRows();
		emit this->valuesChanged();
		emit this->objectInsertedPost(object, index);
	}

	void removeAt(qsizetype index) {
		auto* object = this->mValues.at(index);
		emit this->objectRemovedPre(object, index);
		this->beginRemoveRows(QModelIndex(), static_cast<int>(index), static_cast<int>(index));
		this->mValues.removeAt(index);
		this->endRemoveRows();
		emit this->valuesChanged();
		emit this->objectRemovedPost(object, index);
	}

	QList<QObject*> mValues;
};
