#pragma once
// Qt objects the Rust host drives directly but cxx-qt-lib does not wrap: QApplication, a bare
// QQmlEngine with Quickshell's quit wiring, and root creation through QQmlComponent.
#include <QtCore/QCoreApplication>
#include <QtGui/QGuiApplication>
#include <QtQml/QQmlAbstractUrlInterceptor>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlEngine>
#include <QtWidgets/QApplication>
#include <QtCore/QFile>
#include <QtCore/QLibraryInfo>
#include <QtCore/QTimer>
#include <cstdio>
#include <csignal>
#include <execinfo.h>
#include <unistd.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQml/QQmlContext>
#include <QtQml/qqmldebug.h>
#include <functional>
#include <memory>
#include "rust/cxx.h"
#include "quickshell/include/log.h"
#include "quickshell/include/qsrs.h"
#include "quickshell/include/reload.h"

namespace qsrs {
// Fatal signals print a native backtrace to stderr (the journal under inir.service), then
// re-raise so the exit status and coredump stay the same. Cores of a 600 MB shell exceed
// systemd-coredump's default size limits, so this is often the only trace of a live crash.
inline void crash_handler(int sig) {
	char head[64];
	int n = snprintf(head, sizeof head, "qs-rs: fatal signal %d, backtrace:\n", sig);
	if (n > 0) (void)!write(STDERR_FILENO, head, size_t(n));
	void* frames[64];
	backtrace_symbols_fd(frames, backtrace(frames, 64), STDERR_FILENO);
	signal(sig, SIG_DFL);
	raise(sig);
}

inline void install_crash_handler() {
	// backtrace() loads libgcc lazily: do it now, not inside the handler.
	void* warm[1];
	backtrace(warm, 1);
	for (int sig: {SIGSEGV, SIGBUS, SIGFPE, SIGILL, SIGABRT}) signal(sig, crash_handler);
}

// QML debug/profiler server (qmlprofiler --attach, GammaRay, Qt Creator) on a TCP port.
inline bool start_qml_debug_server(int port, bool block) {
	QQmlDebuggingEnabler::enableDebugging(true);
	return QQmlDebuggingEnabler::startTcpDebugServer(
	    port, block ? QQmlDebuggingEnabler::WaitForClient : QQmlDebuggingEnabler::DoNotWaitForClient);
}

inline rust::String qt_plugins_path() {
	return rust::String(QLibraryInfo::path(QLibraryInfo::PluginsPath).toStdString());
}

// Replaces Qt's plugin search path (before the application exists).
inline void set_plugin_path(const QString& path) { QCoreApplication::setLibraryPaths({path}); }

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

// Measurement aid: QSRS_OBJECT_CENSUS=S prints, every S seconds, live QObjects counted by the QML
// file whose component created them (top 40) and the total.
inline void schedule_census(QQmlEngine* engine) {
	bool ok = false;
	int every = qEnvironmentVariableIntValue("QSRS_OBJECT_CENSUS", &ok);
	if (!ok || every <= 0) return;
	auto* timer = new QTimer(engine);
	QObject::connect(timer, &QTimer::timeout, engine, [] {
		QHash<QString, int> byFile;
		QSet<QObject*> seen;
		std::function<void(QObject*)> walk = [&](QObject* o) {
			if (!o || seen.contains(o)) return;
			seen.insert(o);
			auto* ctx = qmlContext(o);
			auto file = ctx ? ctx->baseUrl().fileName() : QString("<c++> ") + o->metaObject()->className();
			byFile[file]++;
			for (auto* c: o->children()) walk(c);
			if (auto* item = qobject_cast<QQuickItem*>(o)) for (auto* c: item->childItems()) walk(c);
		};
		for (auto* w: QGuiApplication::allWindows()) {
			walk(w);
			if (auto* qw = qobject_cast<QQuickWindow*>(w)) walk(qw->contentItem());
		}
		walk(QCoreApplication::instance());
		QList<QPair<int, QString>> top;
		for (auto it = byFile.begin(); it != byFile.end(); ++it) top.append({it.value(), it.key()});
		std::sort(top.begin(), top.end(), [](auto& a, auto& b) { return a.first > b.first; });
		fprintf(stderr, "qsrs-census total=%lld windows=%lld\n", (long long)seen.size(), (long long)QGuiApplication::allWindows().size());
		for (int i = 0; i < std::min<int>(40, top.size()); i++)
			fprintf(stderr, "qsrs-census %6d %s\n", top[i].first, qPrintable(top[i].second));
	});
	timer->start(every * 1000);
}

inline QQmlEngine* new_engine(const QString& importPath) {
	auto* engine = new QQmlEngine();
	engine->addImportPath(importPath);
	engine->addUrlInterceptor(new RootUrlInterceptor(importPath + "/qs"));
	install_image_providers(*engine);
	install_generation(*engine);
	QObject::connect(engine, &QQmlEngine::quit, QCoreApplication::instance(), &QCoreApplication::quit, Qt::QueuedConnection);
	QObject::connect(engine, &QQmlEngine::exit, QCoreApplication::instance(), &QCoreApplication::exit, Qt::QueuedConnection);
	schedule_census(engine);
	return engine;
}

// Compiles one QML file without creating anything (no singletons, no services) and returns its
// errors, one per line: what upstream only reports once an area gets instantiated.
inline rust::String check_file(QQmlEngine& engine, const QString& path) {
	QQmlComponent component(&engine, QUrl::fromLocalFile(path), QQmlComponent::PreferSynchronous);
	QStringList out;
	for (auto& e: component.errors()) out << e.toString();
	return rust::String(out.join('\n').toStdString());
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
	finish_load(engine, root);
	return root;
}
} // namespace qsrs
