#pragma once
// ScriptModel: a list model over a JS array that turns each new array into row inserts, removes,
// moves and data changes (upstream's diff), so delegates survive updates. Kept in C++ because
// it works on QJSValue and QAbstractListModel signals.
#include <algorithm>
#include <optional>
#include <QtCore/QAbstractListModel>
#include <QtQml/QJSValue>
#include <QtQml/QJSValueIterator>
#include <QtQml/qqml.h>

namespace ObjectComparison {
Q_NAMESPACE;
QML_ELEMENT;
enum Enum : quint8 { Identity = 0, Structure = 1 };
Q_ENUM_NS(Enum);
} // namespace ObjectComparison

class ScriptModel: public QAbstractListModel {
	Q_OBJECT;
	Q_PROPERTY(QList<QJSValue> values READ values WRITE setValues NOTIFY valuesChanged);
	Q_PROPERTY(QString objectProp READ objectProp WRITE setObjectProp NOTIFY objectPropChanged);
	Q_PROPERTY(ObjectComparison::Enum comparisonMode READ comparisonMode WRITE setComparisonMode NOTIFY comparisonModeChanged);
	QML_ELEMENT;

public:
	[[nodiscard]] QList<QJSValue> values() const {
		auto values = this->mValues;
		if (this->isModifying) values.detach();
		return values;
	}

	void setValues(const QList<QJSValue>& newValues) {
		if (this->isModifying) {
			this->stagedValues = newValues;
			return;
		}
		if (this->updateValuesUnique(newValues)) emit this->valuesChanged();
	}

	[[nodiscard]] QString objectProp() const { return this->cmpKey; }
	void setObjectProp(const QString& objectProp) {
		if (objectProp == this->cmpKey) return;
		this->cmpKey = objectProp;
		this->updateValuesUnique(this->mValues);
		emit this->objectPropChanged();
	}

	[[nodiscard]] ObjectComparison::Enum comparisonMode() const { return this->mComparisonMode; }
	void setComparisonMode(ObjectComparison::Enum mode) {
		if (mode == this->mComparisonMode) return;
		this->mComparisonMode = mode;
		emit this->comparisonModeChanged();
	}

	[[nodiscard]] int rowCount(const QModelIndex& parent = QModelIndex()) const override {
		return parent.isValid() ? 0 : static_cast<int>(this->mValues.length());
	}

	[[nodiscard]] QVariant data(const QModelIndex& index, int role) const override {
		if (role != Qt::UserRole) return {};
		return this->mValues.at(index.row()).toVariant(QJSValue::RetainJSObjects);
	}

	[[nodiscard]] QHash<int, QByteArray> roleNames() const override { return {{Qt::UserRole, "modelData"}}; }

signals:
	void valuesChanged();
	void objectPropChanged();
	void comparisonModeChanged();

private:
	struct RecursionPair {
		const QJSValue* a;
		const QJSValue* b;
		const RecursionPair* previous;
	};

	// Deep equality of plain JS objects and arrays; native objects only match by identity.
	static bool structuralEq(const QJSValue& a, const QJSValue& b, const RecursionPair* active = nullptr) {
		if (a.strictlyEquals(b)) return true;
		const auto jsType = QMetaType::fromType<QJSValue>();
		if (a.toVariant(QJSValue::RetainJSObjects).metaType() != jsType) return false;
		if (b.toVariant(QJSValue::RetainJSObjects).metaType() != jsType) return false;
		if (!a.isObject() || !b.isObject() || a.isCallable() || b.isCallable()) return false;
		if (a.isArray() != b.isArray()) return false;
		for (const auto* p = active; p != nullptr; p = p->previous) {
			if (a.strictlyEquals(*p->a) || b.strictlyEquals(*p->b)) return false;
		}
		const RecursionPair next {&a, &b, active};
		QJSValueIterator ai(a);
		QJSValueIterator bi(b);
		while (ai.hasNext()) {
			if (!bi.hasNext()) return false;
			ai.next();
			bi.next();
			if (ai.name() == bi.name()) {
				if (!structuralEq(ai.value(), bi.value(), &next)) return false;
			} else {
				if (!b.hasOwnProperty(ai.name())) return false;
				if (!structuralEq(ai.value(), b.property(ai.name()), &next)) return false;
			}
		}
		return !bi.hasNext();
	}

	bool valueCmp(const QJSValue& a, const QJSValue& b) const {
		if (a.strictlyEquals(b)) return true;
		auto key = [this](const QJSValue& v) { return v.hasProperty(this->cmpKey) ? v.property(this->cmpKey) : v; };
		const auto av = this->cmpKey.isEmpty() ? a : key(a);
		const auto bv = this->cmpKey.isEmpty() ? b : key(b);
		if (this->mComparisonMode == ObjectComparison::Identity) return av.strictlyEquals(bv);
		return structuralEq(av, bv);
	}

	bool updateValuesUnique(const QList<QJSValue>& newValues) {
		auto anyChanges = false;
		this->isModifying = true;
		this->mValues.reserve(newValues.size());
		auto iter = this->mValues.begin();
		auto newIter = newValues.begin();
		auto eq = [this](const QJSValue& b) { return [this, &b](const QJSValue& a) { return this->valueCmp(a, b); }; };
		auto pos = [this](QList<QJSValue>::iterator it) { return static_cast<int>(std::distance(this->mValues.begin(), it)); };

		while (true) {
			if (newIter == newValues.end()) {
				if (iter == this->mValues.end()) break;
				this->beginRemoveRows(QModelIndex(), static_cast<int>(newValues.length()), static_cast<int>(this->mValues.length() - 1));
				this->mValues.erase(iter, this->mValues.end());
				this->endRemoveRows();
				anyChanges = true;
				break;
			} else if (iter == this->mValues.end()) {
				auto start = static_cast<int>(this->mValues.length());
				this->beginInsertRows(QModelIndex(), start, static_cast<int>(newValues.length() - 1));
				this->mValues.append(newValues.sliced(start));
				this->endInsertRows();
				anyChanges = true;
				break;
			} else if (!this->valueCmp(*newIter, *iter)) {
				auto oldIter = std::find_if(iter, this->mValues.end(), eq(*newIter));
				if (oldIter != this->mValues.end()) {
					if (std::find_if(newIter, newValues.end(), eq(*iter)) == newValues.end()) {
						// the current old value is gone: remove the run of gone values
						auto startIter = iter;
						do {
							++iter;
						} while (iter != this->mValues.end() && std::find_if(newIter, newValues.end(), eq(*iter)) == newValues.end());
						auto index = pos(iter);
						auto start = pos(startIter);
						this->beginRemoveRows(QModelIndex(), start, index - 1);
						iter = this->mValues.erase(startIter, iter);
						this->endRemoveRows();
					} else {
						// the new value exists further down: move the matching run up here
						auto oldStart = oldIter;
						do {
							++oldIter;
							++newIter;
						} while (oldIter != this->mValues.end() && newIter != newValues.end() && this->valueCmp(*oldIter, *newIter));
						auto index = pos(iter);
						auto oldStartIndex = pos(oldStart);
						auto oldIndex = pos(oldIter);
						auto len = oldIndex - oldStartIndex;
						this->beginMoveRows(QModelIndex(), oldStartIndex, oldIndex - 1, QModelIndex(), index);
						for (auto i = 0; i != len; i++) this->mValues.move(oldStartIndex + i, index + i);
						iter = this->mValues.begin() + (index + len);
						this->endMoveRows();
					}
				} else {
					// new values not present before: insert the run
					auto startNew = newIter;
					do {
						++newIter;
					} while (newIter != newValues.end() && std::find_if(iter, this->mValues.end(), eq(*newIter)) == this->mValues.end());
					auto index = pos(iter);
					auto len = static_cast<int>(std::distance(startNew, newIter));
					this->beginInsertRows(QModelIndex(), index, index + len - 1);
					this->mValues.insert(index, len, QJSValue());
					std::copy(startNew, newIter, this->mValues.begin() + index);
					iter = this->mValues.begin() + index + len;
					this->endInsertRows();
				}
				anyChanges = true;
			} else if (!newIter->strictlyEquals(*iter)) {
				// equal by key or structure but a different value: update in place
				auto first = pos(iter);
				auto index = first;
				do {
					this->mValues.replace(index, *newIter);
					++iter;
					++newIter;
					++index;
				} while (iter != this->mValues.end() && newIter != newValues.end() && !newIter->strictlyEquals(*iter));
				emit this->dataChanged(this->index(first, 0), this->index(index - 1, 0), {Qt::UserRole});
				anyChanges = true;
			} else {
				++iter;
				++newIter;
			}
		}

		this->isModifying = false;
		if (this->stagedValues.has_value()) {
			auto values = *this->stagedValues;
			this->stagedValues.reset();
			anyChanges |= this->updateValuesUnique(values);
		}
		return anyChanges;
	}

	QList<QJSValue> mValues;
	QString cmpKey;
	ObjectComparison::Enum mComparisonMode = ObjectComparison::Structure;
	bool isModifying = false;
	std::optional<QList<QJSValue>> stagedValues;
};
