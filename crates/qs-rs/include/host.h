#pragma once
// Qt objects the Rust host drives directly but cxx-qt-lib does not wrap: QApplication, a bare
// QQmlEngine with Quickshell's quit wiring, and root creation through QQmlComponent.
#include <QtCore/QCoreApplication>
#include <QtGui/QGuiApplication>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlEngine>
#include <QtWidgets/QApplication>
#include <memory>
#include "rust/cxx.h"
#include "quickshell/include/qsrs.h"

namespace qsrs {
inline std::unique_ptr<QGuiApplication> create_app(bool useQApplication, bool desktopSettingsAware) {
	static int argc = 1;
	static char arg0[] = "quickshell";
	static char* argv[] = {arg0, nullptr};
	QGuiApplication::setDesktopSettingsAware(desktopSettingsAware);
	if (useQApplication) return std::make_unique<QApplication>(argc, argv);
	return std::make_unique<QGuiApplication>(argc, argv);
}

inline int exec_app(QGuiApplication& /*app*/) { return QGuiApplication::exec(); }

inline QQmlEngine* new_engine(const QString& importPath) {
	auto* engine = new QQmlEngine();
	engine->addImportPath(importPath);
	install_image_providers(*engine);
	QObject::connect(engine, &QQmlEngine::quit, QCoreApplication::instance(), &QCoreApplication::quit, Qt::QueuedConnection);
	QObject::connect(engine, &QQmlEngine::exit, QCoreApplication::instance(), &QCoreApplication::exit, Qt::QueuedConnection);
	return engine;
}

// Creates the root object from a local file; prints errors and returns null on failure.
inline QObject* create_root(QQmlEngine& engine, const QString& path) {
	QQmlComponent component(&engine, QUrl::fromLocalFile(path));
	if (component.isError()) {
		for (auto& e: component.errors()) qCritical().noquote() << e.toString();
		return nullptr;
	}
	auto* root = component.create();
	if (root == nullptr) {
		for (auto& e: component.errors()) qCritical().noquote() << e.toString();
		return nullptr;
	}
	QQmlEngine::setObjectOwnership(root, QQmlEngine::CppOwnership);
	return root;
}
} // namespace qsrs
