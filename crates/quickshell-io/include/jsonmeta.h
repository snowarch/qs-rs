#pragma once
// QMetaObject reflection used by JsonAdapter: walks the properties a QML subtype declares on
// top of JsonAdapter/JsonObject, (de)serializes them as JSON and wires their notify signals.
// Mirrors quickshell's jsonadapter.cpp.
#include <QtCore/QByteArray>
#include <QtCore/QDebug>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QMetaContainer>
#include <QtCore/QMetaProperty>
#include <QtCore/QObject>
#include <QtCore/QSequentialIterable>
#include <QtQml/QJSValue>
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlListProperty>
#include <QtQml/qqml.h>

namespace qsrs_io {

// First property index declared below the given C++ base class name.
inline int baseOffset(const QObject* obj, const char* base) {
	for (auto* mo = obj->metaObject(); mo; mo = mo->superClass()) {
		if (qstrcmp(mo->className(), base) == 0) return mo->propertyCount();
	}
	return obj->metaObject()->propertyCount();
}

inline bool isJsonObject(const QObject* obj) {
	for (auto* mo = obj ? obj->metaObject() : nullptr; mo; mo = mo->superClass()) {
		if (qstrcmp(mo->className(), "JsonObject") == 0) return true;
	}
	return false;
}

inline bool isJsonObjectType(QMetaType t) {
	auto* mo = t.metaObject();
	for (; mo; mo = mo->superClass()) {
		if (qstrcmp(mo->className(), "JsonObject") == 0) return true;
	}
	return false;
}

inline QJsonObject serializeRec(const QObject* obj, const char* base) {
	QJsonObject json;
	auto* mo = obj->metaObject();
	for (auto i = baseOffset(obj, base); i < mo->propertyCount(); i++) {
		auto prop = mo->property(i);
		if (!prop.isReadable() || !prop.hasNotifySignal()) continue;
		auto val = prop.read(obj);
		if (val.canView<QObject*>() && (isJsonObjectType(prop.metaType()) || isJsonObject(val.view<QObject*>()))) {
			auto* child = val.view<QObject*>();
			json.insert(prop.name(), child ? QJsonValue(serializeRec(child, "JsonObject")) : QJsonValue::Null);
		} else if (QByteArray(prop.typeName()).startsWith("QQmlListProperty<")) {
			QJsonArray array;
			auto list = QQmlListReference(const_cast<QObject*>(obj), prop.name());
			for (qsizetype j = 0; j < list.count(); j++) {
				auto* child = list.at(j);
				array.push_back(child ? QJsonValue(serializeRec(child, "JsonObject")) : QJsonValue::Null);
			}
			json.insert(prop.name(), array);
		} else {
			if (val.canConvert<QJSValue>()) val = val.value<QJSValue>().toVariant();
			auto jv = QJsonValue::fromVariant(val);
			if (jv.isNull() && !val.isNull() && val.isValid()) {
				if (val.canConvert<QVariantMap>()) val.convert(QMetaType::fromType<QVariantMap>());
				else if (val.canConvert<QVariantList>()) val.convert(QMetaType::fromType<QVariantList>());
				jv = QJsonValue::fromVariant(val);
			}
			json.insert(prop.name(), jv);
		}
	}
	return json;
}

template <typename T>
inline QByteArray json_serialize(const T& adapter) {
	return QJsonDocument(serializeRec(&adapter, "JsonAdapter")).toJson(QJsonDocument::Indented);
}

inline void deserializeRec(QObject* owner, const QJsonObject& json, QObject* obj, const char* base) {
	auto* mo = obj->metaObject();
	for (auto i = baseOffset(obj, base); i < mo->propertyCount(); i++) {
		auto prop = mo->property(i);
		if (!json.contains(prop.name())) continue;
		auto jval = json.value(prop.name());
		if (prop.metaType() == QMetaType::fromType<QVariant>()) {
			auto next = jval.toVariant();
			auto old = prop.read(obj);
			auto oldV = old.canConvert<QJSValue>() ? old.value<QJSValue>().toVariant() : old;
			if (next != oldV) {
				auto* engine = qmlEngine(owner);
				prop.write(obj, engine ? QVariant::fromValue(engine->toScriptValue(next)) : next);
			}
		} else if (isJsonObjectType(prop.metaType())) {
			if (jval.isObject()) {
				auto* current = prop.read(obj).view<QObject*>();
				bool isNew = current == nullptr;
				if (isNew) {
					current = static_cast<QObject*>(prop.metaType().metaObject()->metaType().create());
					if (!current) continue;
					current->setParent(owner);
				}
				deserializeRec(owner, jval.toObject(), current, "JsonObject");
				if (isNew) prop.write(obj, QVariant::fromValue(current));
			} else if (jval.isNull()) {
				prop.write(obj, QVariant::fromValue(nullptr));
			}
		} else if (QByteArray(prop.typeName()).startsWith("QQmlListProperty<")) {
			// Lists of JsonObject are rebuilt from the array; element type comes from existing items.
			auto list = QQmlListReference(obj, prop.name());
			if (!jval.isArray() || !list.isValid()) continue;
			auto array = jval.toArray();
			const QMetaObject* elem = list.listElementType();
			if (!elem) continue;
			if (list.canClear()) list.clear();
			for (const auto& item: array) {
				if (!item.isObject()) continue;
				auto* child = static_cast<QObject*>(elem->metaType().create());
				if (!child) continue;
				child->setParent(owner);
				deserializeRec(owner, item.toObject(), child, "JsonObject");
				list.append(child);
			}
		} else {
			auto variant = jval.toVariant();
			auto v = variant;
			if (prop.metaType().id() != QMetaType::QString && v.metaType().id() != prop.metaType().id()
			    && !v.convert(prop.metaType())) {
				// A typed list (list<real>, list<int>) has no converter from QVariantList: built member by member, as
				// Quickshell does (jsonadapter.cpp). A failed convert left it empty, and the next write saved [].
				auto pval = prop.read(obj);
				if (!variant.canConvert<QSequentialIterable>() || !pval.canView<QSequentialIterable>()) {
					qWarning().nospace() << "JsonAdapter: failed to deserialize property " << prop.name() << ": expected "
					                     << prop.metaType().name() << " but got " << variant.typeName();
					continue;
				}
				v = QVariant(pval.metaType());
				auto target = v.view<QSequentialIterable>().metaContainer();
				auto valueType = target.valueMetaType();
				for (QVariant item: variant.value<QSequentialIterable>())
					if (item.convert(valueType)) target.addValueAtEnd(v.data(), item.constData());
			}
			if (prop.read(obj) != v) prop.write(obj, v);
		}
	}
}

// Returns false (and prints) if data is not a JSON object.
template <typename T>
inline bool json_deserialize(T& adapter, const QByteArray& data) {
	if (data.isEmpty()) return true;
	QJsonParseError error;
	auto json = QJsonDocument::fromJson(data, &error);
	if (error.error != QJsonParseError::NoError) {
		qWarning().noquote() << "JsonAdapter: Failed to deserialize json:" << error.errorString();
		return false;
	}
	if (!json.isObject()) {
		qWarning() << "JsonAdapter: Failed to deserialize json: not an object";
		return false;
	}
	deserializeRec(&adapter, json.object(), &adapter, "JsonAdapter");
	return true;
}

inline void connectRec(QObject& adapter, int slot, QObject* obj, const char* base) {
	auto* mo = obj->metaObject();
	for (auto i = baseOffset(obj, base); i < mo->propertyCount(); i++) {
		auto prop = mo->property(i);
		if (!prop.isReadable() || !prop.hasNotifySignal()) continue;
		QMetaObject::connect(obj, prop.notifySignalIndex(), &adapter, slot, Qt::UniqueConnection);
		auto val = prop.read(obj);
		if (val.canView<QObject*>() && isJsonObjectType(prop.metaType())) {
			if (auto* child = val.view<QObject*>()) connectRec(adapter, slot, child, "JsonObject");
		} else if (QByteArray(prop.typeName()).startsWith("QQmlListProperty<")) {
			auto list = QQmlListReference(obj, prop.name());
			for (qsizetype j = 0; j < list.count(); j++) {
				if (auto* child = list.at(j)) connectRec(adapter, slot, child, "JsonObject");
			}
		}
	}
}

// Connects every serialized property's notify signal to the adapter's `onPropertyChanged()`.
template <typename T>
inline void json_connect_notifiers(T& adapter) {
	auto slot = adapter.metaObject()->indexOfMethod("onPropertyChanged()");
	if (slot != -1) connectRec(adapter, slot, &adapter, "JsonAdapter");
}

} // namespace qsrs_io
