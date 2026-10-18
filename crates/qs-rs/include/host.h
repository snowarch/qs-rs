#pragma once
// Qt objects the Rust host drives directly but cxx-qt-lib does not wrap: QApplication, a bare
// QQmlEngine with Quickshell's quit wiring, and root creation through QQmlComponent.
#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtQml/QQmlAbstractUrlInterceptor>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlEngine>
#include <QtWidgets/QApplication>
#include <QtCore/QFile>
#include <QtCore/QLibraryInfo>
#include <QtCore/QTimer>
#include <QtCore/QAbstractAnimation>
#include <QtCore/QChronoTimer>
#include <QtCore/QElapsedTimer>
#include <QtGui/QScreen>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <csignal>
#include <atomic>
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
#include "quickshell/include/windowpool.h"
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

inline rust::String qt_build_version() { return rust::String(QT_VERSION_STR); }

inline rust::String qt_plugins_path() {
	return rust::String(QLibraryInfo::path(QLibraryInfo::PluginsPath).toStdString());
}

// Replaces Qt's plugin search path (before the application exists).
inline void set_plugin_path(const QString& path) { QCoreApplication::setLibraryPaths({path}); }

// Upstream launch.cpp: the IconTheme pragma, and $XDG_DATA_DIRS/pixmaps as icon fallback paths
// (some programs install their icon there, and the platform themes do not search it).
inline void setup_icons(const QString& theme) {
	if (!theme.isEmpty()) QIcon::setThemeName(theme);
	QStringList dataPaths = qEnvironmentVariable("XDG_DATA_DIRS").split(u':', Qt::SkipEmptyParts);
	if (!qEnvironmentVariableIsSet("XDG_DATA_DIRS")) dataPaths = {"/usr/local/share", "/usr/share"};
	auto fallback = QIcon::fallbackSearchPaths();
	for (const auto& path: dataPaths) {
		auto pixmaps = QDir(path).filePath("pixmaps");
		if (!fallback.contains(pixmaps)) fallback.push_back(pixmaps);
	}
	QIcon::setFallbackSearchPaths(fallback);
}

inline std::unique_ptr<QGuiApplication> create_app(bool useQApplication, bool desktopSettingsAware) {
	static int argc = 1;
	static char arg0[] = "quickshell";
	static char* argv[] = {arg0, nullptr};
	QGuiApplication::setDesktopSettingsAware(desktopSettingsAware);
	if (useQApplication) return std::make_unique<QApplication>(argc, argv);
	return std::make_unique<QGuiApplication>(argc, argv);
}

// The basic render loop (one GL context for every window) installs no animation driver, so QML
// animations step on QUnifiedTimer's fixed 16 ms tick: ~62 steps a second, which judders on a
// 75 or 144 Hz screen. This driver ticks at the fastest screen's refresh rate while any animation
// runs (the ticks are what makes animated windows ask for frames) and sleeps otherwise. Each
// frame (an update request, paced by the compositor's frame callbacks) also brings animations
// to its own time before it is built: a free-running timer drifts against the real vblank, and
// 1 Hz apart was a doubled or missing step every ~0.8 s.
class DisplayAnimationDriver: public QAnimationDriver {
public:
	explicit DisplayAnimationDriver(QObject* parent): QAnimationDriver(parent) {
		mTimer.setTimerType(Qt::PreciseTimer);
		QObject::connect(&mTimer, &QChronoTimer::timeout, this, [this] { tick(); });
		auto watch = [this](QScreen* s) {
			QObject::connect(s, &QScreen::refreshRateChanged, this, [this] { updateInterval(); });
			updateInterval();
		};
		for (auto* s: QGuiApplication::screens()) watch(s);
		QObject::connect(qApp, &QGuiApplication::screenAdded, this, watch);
		QObject::connect(qApp, &QGuiApplication::screenRemoved, this, [this] { updateInterval(); });
		qApp->installEventFilter(this);
	}

	bool eventFilter(QObject* watched, QEvent* event) override {
		// Windows drawing in the same refresh share one step.
		if (event->type() == QEvent::UpdateRequest && isRunning() && watched->isWindowType()
		    && mClock.elapsed() - mLastTick >= 2)
			tick();
		return false;
	}

	qint64 elapsed() const override { return isRunning() ? mClock.elapsed() : 0; }

protected:
	void start() override {
		mClock.start();
		mLastTick = 0;
		QAnimationDriver::start();
		mTimer.start();
	}
	void stop() override {
		mTimer.stop();
		QAnimationDriver::stop();
	}

private:
	void tick() {
		mLastTick = mClock.elapsed();
		advance();
	}

	void updateInterval() {
		qreal hz = 60;
		for (auto* s: QGuiApplication::screens()) hz = std::max(hz, s->refreshRate());
		// Tests have no real screen: QSRS_ANIMATION_HZ stands in for one.
		if (auto forced = qEnvironmentVariableIntValue("QSRS_ANIMATION_HZ"); forced > 0) hz = forced;
		mTimer.setInterval(std::chrono::nanoseconds(qint64(1e9 / hz)));
	}

	QChronoTimer mTimer;
	QElapsedTimer mClock;
	qint64 mLastTick = 0;
};

// Upstream launch.cpp after the application exists: the app id surfaces carry, transparent
// windows, NativeTextRendering, and hidden windows never quit the shell.
inline void configure_app(const QString& appId, bool nativeTextRendering) {
	QGuiApplication::setDesktopFileName(appId);
	QQuickWindow::setDefaultAlphaBuffer(true);
	if (nativeTextRendering) QQuickWindow::setTextRenderType(QQuickWindow::NativeTextRendering);
	QGuiApplication::setQuitOnLastWindowClosed(false);
	// QSRS_QT_ANIMATION_TIMER=1 keeps Qt's 16 ms tick (A/B).
	if (!qEnvironmentVariableIsSet("QSRS_PER_WINDOW_GL") && !qEnvironmentVariableIsSet("QSRS_QT_ANIMATION_TIMER")) {
		(new DisplayAnimationDriver(qApp))->install();
	}
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
// file whose component created them (top 40), the total and every window with its object count.
// QSRS_OBJECT_CENSUS=signal prints one census on each SIGUSR1.
inline std::atomic<int> censusRequests {0};
inline void census_signal(int) { censusRequests.fetch_add(1); }
inline std::atomic<int> reclaimRequests {0};
inline void reclaim_signal(int) { reclaimRequests.fetch_add(1); }
extern "C" int mallctl(const char* name, void* oldp, size_t* oldlenp, void* newp, size_t newlen);

// One line of where memory sits: process RSS, jemalloc live/resident/retained bytes (MB).
inline void print_memory(const char* stage) {
	long pages = 0, rss = 0;
	if (FILE* f = fopen("/proc/self/statm", "r")) { if (fscanf(f, "%ld %ld", &pages, &rss) != 2) rss = 0; fclose(f); }
	uint64_t epoch = 1;
	size_t len = sizeof epoch;
	mallctl("epoch", &epoch, &len, &epoch, len);
	auto stat = [](const char* name) { size_t v = 0, l = sizeof v; return mallctl(name, &v, &l, nullptr, 0) == 0 ? v >> 20 : 0; };
	fprintf(stderr, "qsrs-memory %-10s rss=%ldMB allocated=%zuMB active=%zuMB resident=%zuMB retained=%zuMB\n", stage,
	        rss * sysconf(_SC_PAGESIZE) >> 20, stat("stats.allocated"), stat("stats.active"), stat("stats.resident"),
	        stat("stats.retained"));
}

// Measurement aid (SIGUSR2 under QSRS_OBJECT_CENSUS=signal): what each reclaim step gives back.
inline void reclaim_steps(QQmlEngine* engine) {
	print_memory("before");
	engine->collectGarbage();
	print_memory("gc");
	engine->trimComponentCache();
	engine->collectGarbage();
	print_memory("trim");
	QQuickWindow* any = nullptr;
	for (auto* w: QGuiApplication::allWindows()) if ((any = qobject_cast<QQuickWindow*>(w))) any->releaseResources();
	print_memory("release");
	mallctl("arena.4096.purge", nullptr, nullptr, nullptr, 0);
	print_memory("purge");
}

inline void print_census() {
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
	QStringList windows;
	for (auto* w: QGuiApplication::allWindows()) {
		auto before = seen.size();
		walk(w);
		if (auto* qw = qobject_cast<QQuickWindow*>(w)) walk(qw->contentItem());
		auto* ctx = qmlContext(w);
		windows << QString("%1%2:%3").arg(w->isVisible() ? "" : "-", ctx ? ctx->baseUrl().fileName() : w->metaObject()->className()).arg(seen.size() - before);
	}
	walk(QCoreApplication::instance());
	QList<QPair<int, QString>> top;
	for (auto it = byFile.begin(); it != byFile.end(); ++it) top.append({it.value(), it.key()});
	std::sort(top.begin(), top.end(), [](auto& a, auto& b) { return a.first > b.first; });
	fprintf(stderr, "qsrs-census total=%lld windows=%lld\n", (long long)seen.size(), (long long)QGuiApplication::allWindows().size());
	fprintf(stderr, "qsrs-census windows %s\n", qPrintable(windows.join(' ')));
	for (int i = 0; i < std::min<int>(40, top.size()); i++)
		fprintf(stderr, "qsrs-census %6d %s\n", top[i].first, qPrintable(top[i].second));
}

// Memory goes back after surfaces close. Freed QML objects leave garbage in the JS heap until an
// allocation-driven GC, and jemalloc keeps the freed pages dirty (its decay did not return them
// while the shell idles): measured after opening and closing every inir panel, RSS stayed 160 MB
// over start with the object count back at its start value. A hidden window starts two passes:
// one at 6 s (the surface is gone) and one at 26 s (inir unloads closed panels ~20 s later).
// Each pass: JS GC, unused compiled components, the scene graph's caches of hidden windows,
// then a jemalloc purge. QSRS_NO_RECLAIM=1 turns it off (A/B).
inline void reclaim(QQmlEngine* engine) {
	engine->collectGarbage();
	engine->trimComponentCache();
	for (auto* w: QGuiApplication::allWindows())
		if (auto* qw = qobject_cast<QQuickWindow*>(w); qw && !qw->isVisible()) qw->releaseResources();
	mallctl("arena.4096.purge", nullptr, nullptr, nullptr, 0);
}

// Watches window Hide events (no polling: an idle shell is never woken for this).
class ReclaimWatcher: public QObject {
public:
	explicit ReclaimWatcher(QQmlEngine* engine): QObject(engine), engine(engine) {
		for (auto* t: {&this->early, &this->late}) {
			t->setSingleShot(true);
			QObject::connect(t, &QTimer::timeout, this, [this] { reclaim(this->engine); });
		}
		this->early.setInterval(6000);
		this->late.setInterval(26000);
		qApp->installEventFilter(this);
	}

protected:
	bool eventFilter(QObject* o, QEvent* e) override {
		if (e->type() == QEvent::Hide && o->isWindowType()) {
			this->early.start();
			this->late.start();
		}
		return false;
	}

private:
	QQmlEngine* engine;
	QTimer early;
	QTimer late;
};

inline void schedule_reclaim(QQmlEngine* engine) {
	if (qEnvironmentVariableIntValue("QSRS_NO_RECLAIM") == 1) return;
	new ReclaimWatcher(engine);
}

inline void schedule_census(QQmlEngine* engine) {
	auto mode = qEnvironmentVariable("QSRS_OBJECT_CENSUS");
	if (mode.isEmpty()) return;
	auto* timer = new QTimer(engine);
	if (mode == "signal") {
		signal(SIGUSR1, census_signal);
		signal(SIGUSR2, reclaim_signal);
		QObject::connect(timer, &QTimer::timeout, engine, [engine] {
			while (censusRequests.load() > 0) {
				censusRequests.fetch_sub(1);
				print_census();
			}
			while (reclaimRequests.load() > 0) {
				reclaimRequests.fetch_sub(1);
				reclaim_steps(engine);
			}
		});
		timer->start(100);
		return;
	}
	int every = mode.toInt();
	if (every <= 0) return;
	QObject::connect(timer, &QTimer::timeout, engine, [] { print_census(); });
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
	schedule_reclaim(engine);
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
rust::String rescan_vfs() noexcept;

// Hot reload, upstream RootWrapper::reloadGraph: re-scan into the VFS, compile the shell in a new
// engine (new generation: fresh singletons), and only when it compiles replace the running one;
// otherwise the old shell keeps running and gets reloadFailed. The old root goes first, so the
// services it owns release their D-Bus names and paths before the new ones claim them.
class QsShell: public QObject {
public:
	QsShell(QQmlEngine* engine, QObject* root, QString importPath, QString entry)
	    : engine(engine), root(root), importPath(std::move(importPath)), entry(std::move(entry)) {}

	void reload(bool hard) {
		Q_UNUSED(hard); // every reload is a new generation; shown windows are carried over (QsWindowPool)
		if (this->reloading) return;
		this->reloading = true;
		this->doReload();
		this->reloading = false;
	}

	static QsShell* instance;

private:
	static QObject* quickshell(QQmlEngine* engine) {
		return engine == nullptr ? nullptr : engine->singletonInstance<QObject*>("Quickshell", "Quickshell");
	}

	void failed(const QString& error) {
		qCritical().noquote() << error;
		if (auto* qs = quickshell(this->engine)) QMetaObject::invokeMethod(qs, "reloadFailed", Q_ARG(QString, error));
	}

	void doReload() {
		auto scanError = rescan_vfs();
		if (!scanError.empty()) {
			this->failed("Failed to load configuration\n  " + QString::fromStdString(std::string(scanError)));
			return;
		}
		qInfo() << "Reloading configuration...";
		auto* next = new_engine(this->importPath);
		auto* component = new QQmlComponent(next, QUrl::fromLocalFile(this->entry), QQmlComponent::PreferSynchronous);
		if (component->isError()) {
			QString error = "Failed to load configuration";
			for (auto& e: component->errors()) error += "\n  caused by " + e.toString();
			delete component;
			delete next;
			this->failed(error);
			return;
		}

		// Shown windows outlive the old generation, frozen on their last frame, for the new
		// proxies to take back (QsWindowPool).
		QsWindowPool::stash();
		// The old generation: root first (its children may still reach the engine), then the engine.
		delete this->root;
		this->root = nullptr;
		QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
		this->engine->collectGarbage();
		delete this->engine;
		this->engine = next;

		auto* root = component->create();
		delete component;
		if (root == nullptr) {
			QsWindowPool::dropNow();
			qCritical() << "Failed to create the reloaded configuration";
			return;
		}
		QQmlEngine::setObjectOwnership(root, QQmlEngine::CppOwnership);
		this->root = root;
		finish_load(*next, root);
		QsWindowPool::drop();
		qInfo() << "Configuration Loaded";
		if (auto* qs = quickshell(next)) QMetaObject::invokeMethod(qs, "reloadCompleted");
	}

	QQmlEngine* engine;
	QObject* root;
	QString importPath;
	QString entry;
	bool reloading = false;
};
inline QsShell* QsShell::instance = nullptr;

inline void install_reload(QQmlEngine* engine, QObject* root, const QString& importPath, const QString& entry) {
	QsShell::instance = new QsShell(engine, root, importPath, entry);
}

// Quickshell.reload() runs inside the old generation's JS (or while its root is still being
// created): reload from the event loop once that has returned.
inline void request_reload(bool hard) {
	QMetaObject::invokeMethod(QCoreApplication::instance(), [hard]() {
		if (QsShell::instance != nullptr) QsShell::instance->reload(hard);
	}, Qt::QueuedConnection);
}
} // namespace qsrs
