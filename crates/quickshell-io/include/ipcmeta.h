#pragma once
// Calls a QML function declared on an IpcHandler, converting string arguments to the declared
// parameter types (string, int, bool, real, color), like quickshell's ipchandler.cpp.
#include <QtCore/QMetaMethod>
#include <QtCore/QObject>
#include <QtGui/QColor>
#include <QtQml/QJSEngine>
#include <QtQml/QJSValue>
#include <QtQml/QQmlEngine>
#include "rust/cxx.h"

namespace qsrs_io {

// First method index declared by the QML subtype, below the IpcHandler class itself.
inline int ipcOffset(const QObject& obj) {
	for (auto* mo = obj.metaObject(); mo; mo = mo->superClass()) {
		if (qstrcmp(mo->className(), "IpcHandler") == 0) return mo->methodCount();
	}
	return QObject::staticMetaObject.methodCount();
}

inline QMetaMethod findIpcFunction(const QObject& obj, const QString& name) {
	auto* mo = obj.metaObject();
	for (auto i = ipcOffset(obj); i < mo->methodCount(); i++) {
		auto m = mo->method(i);
		auto t = m.methodType();
		if ((t == QMetaMethod::Method || t == QMetaMethod::Slot) && QString::fromLatin1(m.name()) == name) return m;
	}
	return {};
}

inline QString typeLabel(QMetaType t) {
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

// Returns 0 on success with the stringified result in `out`; otherwise an error message in `out`.
template <typename T>
inline int ipc_call(T& handler, const QString& name, const QStringList& args, QString& out) {
	auto method = findIpcFunction(handler, name);
	if (!method.isValid()) {
		out = "Function " + name + " not found.";
		return 1;
	}
	if (method.parameterCount() != args.size()) {
		out = QString("Function %1 takes %2 arguments, %3 given.").arg(name).arg(method.parameterCount()).arg(args.size());
		return 2;
	}
	auto* engine = qmlEngine(&handler);
	if (!engine) {
		out = "Handler has no engine.";
		return 3;
	}
	QJSValueList jsArgs;
	for (int i = 0; i < args.size(); i++) {
		auto& a = args[i];
		switch (method.parameterMetaType(i).id()) {
		case QMetaType::Int: {
			bool ok = false;
			auto v = a.toInt(&ok);
			if (!ok) { out = "Argument " + a + " is not an int."; return 2; }
			jsArgs << QJSValue(v);
			break;
		}
		case QMetaType::Double: {
			bool ok = false;
			auto v = a.toDouble(&ok);
			if (!ok) { out = "Argument " + a + " is not a real."; return 2; }
			jsArgs << QJSValue(v);
			break;
		}
		case QMetaType::Bool:
			if (a != "true" && a != "false") { out = "Argument " + a + " is not a bool."; return 2; }
			jsArgs << QJSValue(a == "true");
			break;
		case QMetaType::QColor: jsArgs << engine->toScriptValue(QColor::fromString(a)); break;
		default: jsArgs << QJSValue(a);
		}
	}
	auto self = engine->toScriptValue(static_cast<QObject*>(&handler));
	auto result = self.property(name).callWithInstance(self, jsArgs);
	if (result.isError()) {
		out = result.toString();
		return 4;
	}
	out = result.isUndefined() || result.isNull() ? QString() : result.toString();
	return 0;
}

// "function name(arg: type): ret" lines for `ipc show`.
template <typename T>
inline QString ipc_describe(const T& handler) {
	QString s;
	auto* mo = handler.metaObject();
	for (auto i = ipcOffset(handler); i < mo->methodCount(); i++) {
		auto m = mo->method(i);
		if ((m.methodType() != QMetaMethod::Method && m.methodType() != QMetaMethod::Slot) || m.name().startsWith("_")) continue;
		QStringList params;
		for (int p = 0; p < m.parameterCount(); p++) {
			params << QString::fromLatin1(m.parameterNames().value(p)) + ": " + typeLabel(m.parameterMetaType(p));
		}
		s += "  function " + QString::fromLatin1(m.name()) + "(" + params.join(", ") + "): " + typeLabel(m.returnMetaType()) + "\n";
	}
	return s;
}

} // namespace qsrs_io
