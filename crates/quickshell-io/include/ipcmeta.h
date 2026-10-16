#pragma once
// QMetaObject side of IpcHandler (upstream io/ipchandler.cpp, io/ipc.cpp): the functions,
// properties and signals a QML IpcHandler declares, calls with string arguments converted to the
// declared types (string, int, bool, real, color), and results/properties as strings.
#include <QtCore/QCoreApplication>
#include <QtCore/QMetaMethod>
#include <QtCore/QMetaProperty>
#include <QtCore/QObject>
#include <QtGui/QColor>
#include <QtQml/QJSEngine>
#include <QtQml/QJSValue>
#include <QtQml/QQmlEngine>
#include "rust/cxx.h"

namespace qsrs_io {

// The IpcHandler class itself, so only what the QML subtype declares is exposed.
inline const QMetaObject* ipcBase(const QObject& obj) {
	for (auto* mo = obj.metaObject(); mo; mo = mo->superClass()) {
		if (qstrcmp(mo->className(), "IpcHandler") == 0) return mo;
	}
	return &QObject::staticMetaObject;
}

inline QMetaMethod findIpcFunction(const QObject& obj, const QString& name) {
	auto* mo = obj.metaObject();
	for (auto i = ipcBase(obj)->methodCount(); i < mo->methodCount(); i++) {
		auto m = mo->method(i);
		if (m.methodType() == QMetaMethod::Slot && QString::fromLatin1(m.name()) == name) return m;
	}
	return {};
}

inline QString typeName(QMetaType t) {
	switch (t.id()) {
	case QMetaType::QString: return "string";
	case QMetaType::Int: return "int";
	case QMetaType::Bool: return "bool";
	case QMetaType::Double: return "real";
	case QMetaType::QColor: return "color";
	case QMetaType::Void: case QMetaType::UnknownType: return "void";
	default: return QString::fromLatin1(t.name());
	}
}

// upstream IpcValueSlot::toString
inline QString valueString(const QVariant& v) {
	switch (v.typeId()) {
	case QMetaType::QString: return v.value<QString>();
	case QMetaType::Int: return QString::number(v.value<int>());
	case QMetaType::Bool: return v.value<bool>() ? "true" : "false";
	case QMetaType::Double: return QString::number(v.value<double>());
	case QMetaType::QColor: return v.value<QColor>().name(QColor::HexArgb);
	default: return v.isValid() ? v.toString() : QString();
	}
}

// One entry per line, fields separated by \x1f: "f name ret argname argtype ...",
// "p name type", "s name [argname argtype]".
template <typename T>
inline QStringList ipc_describe(const T& handler) {
	QStringList out;
	auto* mo = handler.metaObject();
	auto* base = ipcBase(handler);
	for (auto i = base->methodCount(); i < mo->methodCount(); i++) {
		auto m = mo->method(i);
		if (m.name().startsWith("_")) continue;
		QStringList f;
		if (m.methodType() == QMetaMethod::Slot) {
			f << "f" << QString::fromLatin1(m.name()) << typeName(m.returnMetaType());
		} else if (m.methodType() == QMetaMethod::Signal && m.parameterCount() <= 1) {
			f << "s" << QString::fromLatin1(m.name());
		} else {
			continue;
		}
		for (int p = 0; p < m.parameterCount(); p++) {
			f << QString::fromLatin1(m.parameterNames().value(p)) << typeName(m.parameterMetaType(p));
		}
		out << f.join(QChar(0x1f));
	}
	for (auto i = base->propertyCount(); i < mo->propertyCount(); i++) {
		auto p = mo->property(i);
		if (!p.isReadable() || !p.hasNotifySignal() || QByteArray(p.name()).startsWith("_")) continue;
		out << QStringList {"p", QString::fromLatin1(p.name()), typeName(p.metaType())}.join(QChar(0x1f));
	}
	return out;
}

// 0: called, result string in `out`. -1: no such function. -2: wrong argument count.
// n > 0: argument n - 1 could not be parsed as its declared type.
template <typename T>
inline int ipc_call(T& handler, const QString& name, const QStringList& args, QString& out) {
	auto method = findIpcFunction(handler, name);
	if (!method.isValid()) return -1;
	if (method.parameterCount() != args.size()) return -2;
	auto* engine = qmlEngine(&handler);
	if (!engine) return -1;
	QJSValueList jsArgs;
	for (int i = 0; i < args.size(); i++) {
		const auto& a = args[i];
		bool ok = true;
		switch (method.parameterMetaType(i).id()) {
		case QMetaType::Int: jsArgs << QJSValue(a.toInt(&ok)); break;
		case QMetaType::Double: jsArgs << QJSValue(a.toDouble(&ok)); break;
		case QMetaType::Bool:
			ok = a == "true" || a == "false";
			jsArgs << QJSValue(a == "true");
			break;
		case QMetaType::QColor: {
			auto c = QColor::fromString(a);
			ok = c.isValid();
			jsArgs << engine->toScriptValue(c);
			break;
		}
		default: jsArgs << QJSValue(a);
		}
		if (!ok) return i + 1;
	}
	auto self = engine->toScriptValue(static_cast<QObject*>(&handler));
	auto result = self.property(name).callWithInstance(self, jsArgs);
	if (result.isError()) {
		qWarning().noquote() << "IpcHandler:" << result.toString();
		out = QString();
		return 0;
	}
	auto variant = result.toVariant();
	auto ret = method.returnMetaType();
	if (ret.isValid() && ret.id() != QMetaType::Void && ret.id() != QMetaType::QVariant && variant.canConvert(ret)) {
		variant.convert(ret);
	}
	out = result.isUndefined() || result.isNull() ? QString() : valueString(variant);
	return 0;
}

// Reads a declared property as a string; false when there is none.
template <typename T>
inline bool ipc_read_property(const T& handler, const QString& name, QString& out) {
	auto* mo = handler.metaObject();
	for (auto i = ipcBase(handler)->propertyCount(); i < mo->propertyCount(); i++) {
		auto p = mo->property(i);
		if (QString::fromLatin1(p.name()) == name && p.isReadable()) {
			out = valueString(p.read(&handler));
			return true;
		}
	}
	return false;
}

// IPC kill (upstream IpcKillCommand): quit the event loop from any thread.
inline void ipc_quit() { QMetaObject::invokeMethod(QCoreApplication::instance(), &QCoreApplication::quit, Qt::QueuedConnection); }

} // namespace qsrs_io
