#pragma once
// Qt objects the Rust host drives directly but cxx-qt-lib does not wrap: QApplication, a bare
// QQmlEngine with Quickshell's quit wiring, and root creation through QQmlComponent.
#include <QtCore/QCoreApplication>
#include <QtGui/QGuiApplication>
#include <QtQml/QQmlAbstractUrlInterceptor>
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

// Quickshell's `root:` URLs (shell-relative imports and paths, e.g. `import "root:services"`)
// point into the shell's VFS mirror, like upstream's qs:@/qs/ rewrite.
class RootUrlInterceptor: public QQmlAbstractUrlInterceptor {
public:
	explicit RootUrlInterceptor(QString root): root(std::move(root)) {}

	QUrl intercept(const QUrl& url, DataType /*type*/) override {
		QString path;
		if (url.scheme() == "root") path = url.path();
		else if (url.scheme() == "qs" && url.path().startsWith("@/qs/")) path = url.path().sliced(5);
		else return url;
		while (path.startsWith('/')) path = path.sliced(1);
		auto out = QUrl::fromLocalFile(path.isEmpty() ? this->root : this->root + "/" + path);
		out.setQuery(url.query());
		out.setFragment(url.fragment());
		return out;
	}

private:
	QString root;
};

inline QQmlEngine* new_engine(const QString& importPath) {
	auto* engine = new QQmlEngine();
	engine->addImportPath(importPath);
	engine->addUrlInterceptor(new RootUrlInterceptor(importPath + "/qs"));
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
