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
#include <QtDBus/QDBusConnection>
#include <QtDBus/QDBusMessage>
#include <QtCore/QLibrary>
#include <QtCore/QVersionNumber>
#include <QtCore/QLibraryInfo>
#include <QtCore/QTimer>
#include <QtCore/QAbstractAnimation>
#include <QtCore/QChronoTimer>
#include <QtCore/QElapsedTimer>
#include <QtGui/QScreen>
#include <QtGui/QFont>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <csignal>
#include <atomic>
#include <execinfo.h>
#include <fcntl.h>
#include <QtCore/QSocketNotifier>
#include <QtCore/QDateTime>
#include <csetjmp>
#include <cstring>
#include <unistd.h>
#include <sys/mman.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQml/QQmlContext>
#include <QtQml/qqmldebug.h>
#include <functional>
#include <memory>
#include "rust/cxx.h"

// Qt's hook table for debuggers (qhooks_p.h, exported by QtCore; index 4 is RemoveQObject, a
// stable slot of the hook ABI).
QT_BEGIN_NAMESPACE
extern Q_CORE_EXPORT quintptr qtHookData[];
QT_END_NAMESPACE
#include "quickshell/include/log.h"
#include "quickshell/include/qsrs.h"
#include "quickshell/include/windowpool.h"
#include "quickshell/include/reload.h"
#include "frames.h"
#include "ui.h"

// QtQml's debugger helper (extern "C", exported): the JS/QML call stack of a V4 engine as text,
// read from the engine's own frame chain, so JIT frames the native unwinder stops at still show.
extern "C" char* qt_v4StackTraceForEngine(void* executionEngine);

namespace qsrs {
/// The V4 engines of the live QML engines (the running generation and, during a reload, the
/// one being torn down), whose JS stacks a fatal signal prints. Fixed slots: the handler reads
/// them without locks.
inline std::atomic<void*> crash_engines[4] {};

inline void crash_engine_add(void* v4) {
	for (auto& slot: crash_engines) {
		void* empty = nullptr;
		if (slot.compare_exchange_strong(empty, v4)) return;
	}
}

inline void crash_engine_remove(void* v4) {
	for (auto& slot: crash_engines) {
		void* expected = v4;
		slot.compare_exchange_strong(expected, nullptr);
	}
}

/// One line per frame, `  #0 func  path:line`, from Qt's gdb/MI text
/// (`frame={level="0",func="spin",file="...",line="8",...}`). Paths lose the runtime copy's
/// prefix (`.../vfs/<hash>/qs/`), leaving the one inside the shell.
inline void write_qml_stack(const char* mi) {
	bool any = false;
	auto field = [](const char* from, const char* end, const char* key, const char** start) -> size_t {
		const char* at = strstr(from, key);
		if (!at || at >= end) return 0;
		*start = at + strlen(key);
		const char* close = strchr(*start, '"');
		return close && close < end ? size_t(close - *start) : 0;
	};
	for (const char* frame = strstr(mi, "frame={"); frame; frame = strstr(frame + 1, "frame={")) {
		const char* end = strchr(frame, '}');
		if (!end) break;
		const char *level = "", *func = "", *file = "", *line = "";
		size_t nl = field(frame, end, "level=\"", &level), nf = field(frame, end, "func=\"", &func);
		size_t nfile = field(frame, end, "file=\"", &file), nline = field(frame, end, "line=\"", &line);
		if (const char* vfs = strstr(file, "/vfs/"); vfs && vfs < file + nfile) {
			if (const char* qs = strstr(vfs, "/qs/"); qs && qs < file + nfile) {
				nfile -= size_t(qs + 4 - file);
				file = qs + 4;
			}
		}
		(void)!write(STDERR_FILENO, "  #", 3);
		(void)!write(STDERR_FILENO, level, nl);
		(void)!write(STDERR_FILENO, " ", 1);
		(void)!write(STDERR_FILENO, func, nf);
		(void)!write(STDERR_FILENO, "  ", 2);
		(void)!write(STDERR_FILENO, file, nfile);
		(void)!write(STDERR_FILENO, ":", 1);
		(void)!write(STDERR_FILENO, line, nline);
		(void)!write(STDERR_FILENO, "\n", 1);
		any = true;
	}
	if (!any) {
		static const char none[] = "  (no frames)\n";
		(void)!write(STDERR_FILENO, none, sizeof none - 1);
	}
}

inline void write_str(const char* text) { (void)!write(STDERR_FILENO, text, strlen(text)); }

// Set while the handler reads a JS stack: a fault or a hang in there (an engine half torn
// down, a heap lock held by the crashing code) jumps back out instead of killing or freezing
// the process before it can restart.
inline sigjmp_buf crash_walk_jump;
inline volatile sig_atomic_t crash_walking = 0;

// The JS stacks of every engine that is running JS. qt_v4StackTraceForEngine reads the
// engine's current frame without checking for one, so an idle engine is skipped first:
// EngineBase starts with `CppStackFrame* currentStackFrame` (qv4enginebase_p.h).
inline void write_qml_stacks() {
	for (auto& slot: crash_engines) {
		void* v4 = slot.load(std::memory_order_acquire);
		if (v4 == nullptr) continue;
		if (sigsetjmp(crash_walk_jump, 1) != 0) {
			crash_walking = 0;
			alarm(0);
			write_str("  (unreadable: the engine faulted or hung while reading it)\n");
			continue;
		}
		crash_walking = 1;
		alarm(2);
		void* frame = *static_cast<void* volatile*>(v4);
		if (frame == nullptr) {
			write_str("  (engine not running JS)\n");
		} else if (char* trace = qt_v4StackTraceForEngine(v4)) {
			write_qml_stack(trace);
		}
		alarm(0);
		crash_walking = 0;
	}
}

// Fatal signals print a native backtrace to stderr (the journal under inir.service), then the
// QML engines' JS stacks, then re-raise so the exit status and coredump stay the same. Cores of
// a 600 MB shell exceed systemd-coredump's default size limits, so this is often the only trace
// of a live crash. Reading a JS stack is not async-signal-safe, so it goes last and runs under
// crash_walking: the handler is not deferred while it runs, so a second fault lands here again
// and jumps out, and SIGALRM ends a walk that blocks.
inline void crash_handler(int sig) {
	if (crash_walking) siglongjmp(crash_walk_jump, 1);
	static volatile sig_atomic_t entered = 0;
	if (entered || sig == SIGALRM) {
		signal(sig, SIG_DFL);
		raise(sig);
		return;
	}
	entered = 1;
	char head[64];
	int n = snprintf(head, sizeof head, "qs-rs: fatal signal %d, backtrace:\n", sig);
	if (n > 0) (void)!write(STDERR_FILENO, head, size_t(n));
	void* frames[64];
	backtrace_symbols_fd(frames, backtrace(frames, 64), STDERR_FILENO);
	write_str("qs-rs: QML stack (innermost first):\n");
	struct sigaction alarm_action {};
	alarm_action.sa_handler = crash_handler;
	alarm_action.sa_flags = SA_NODEFER | SA_ONSTACK;
	sigaction(SIGALRM, &alarm_action, nullptr);
	write_qml_stacks();
	signal(sig, SIG_DFL);
	raise(sig);
}

inline void install_crash_handler() {
	// backtrace() loads libgcc lazily: do it now, not inside the handler.
	void* warm[1];
	backtrace(warm, 1);
	// A blown native stack (deep recursion) would leave the handler no room to run.
	static char alt[256 * 1024];
	stack_t stack {};
	stack.ss_sp = alt;
	stack.ss_size = sizeof alt;
	sigaltstack(&stack, nullptr);
	struct sigaction action {};
	action.sa_handler = crash_handler;
	action.sa_flags = SA_NODEFER | SA_ONSTACK;
	sigemptyset(&action.sa_mask);
	for (int sig: {SIGSEGV, SIGBUS, SIGFPE, SIGILL, SIGABRT}) sigaction(sig, &action, nullptr);
}

// SIGTERM (systemd stopping or restarting inir) and SIGINT quit the event loop instead of
// killing the process, so the shell's exit path runs (niri drops its window recordings). The
// signal reaches the Qt thread through a pipe. A second signal, or an exit stuck for 3 s, ends
// the process the default way.
inline int quit_pipe[2] = {-1, -1};

inline void quit_signal(int sig) {
	signal(sig, SIG_DFL);
	char byte = char(sig);
	(void)!write(quit_pipe[1], &byte, 1);
}

inline void install_quit_signals() {
	if (pipe2(quit_pipe, O_CLOEXEC | O_NONBLOCK) != 0) return;
	auto* notifier = new QSocketNotifier(quit_pipe[0], QSocketNotifier::Read, QCoreApplication::instance());
	QObject::connect(notifier, &QSocketNotifier::activated, notifier, [notifier] {
		char byte = 0;
		(void)!read(quit_pipe[0], &byte, 1);
		notifier->setEnabled(false);
		qInfo().noquote() << "Quitting on" << sigabbrev_np(byte);
		alarm(3);
		QCoreApplication::quit();
	});
	struct sigaction action {};
	action.sa_handler = quit_signal;
	sigemptyset(&action.sa_mask);
	for (int sig: {SIGTERM, SIGINT}) sigaction(sig, &action, nullptr);
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

// The desktop's icon theme when no platform theme gave one: the bundle's own Qt cannot load the desktop's
// platform theme plugin (kde, qt6ct), and Qt's generic theme only knows hicolor.
inline void setup_desktop_icons(const QString& theme) {
	auto current = QIcon::themeName();
	if (current.isEmpty() || current == QStringLiteral("hicolor")) QIcon::setThemeName(theme);
}

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

// The EnableQtWebEngineQuick pragma: QtWebEngineQuick::initialize() before the application
// exists (Qt 6.8 and older built Chromium's shared GL context there; 6.9+ only checks the graphics
// API). Resolved at runtime so only shells that ask load it, and a system without it still starts.
inline bool init_webengine() {
	// Measured: loading it costs 23 ms and 27 MB at every start, for nothing since 6.9.
	if (QLibraryInfo::version() >= QVersionNumber(6, 9)) return true;
	QLibrary lib("Qt6WebEngineQuick", 6);
	auto* init = reinterpret_cast<void (*)()>(lib.resolve("_ZN16QtWebEngineQuick10initializeEv"));
	if (!init) {
		qWarning().noquote() << "EnableQtWebEngineQuick: could not load QtWebEngineQuick:" << lib.errorString();
		return false;
	}
	init();
	return true;
}

inline std::unique_ptr<QGuiApplication> create_app(const QString& appId, bool useQApplication, bool desktopSettingsAware) {
	static int argc = 1;
	static char arg0[] = "quickshell";
	static char* argv[] = {arg0, nullptr};
	QGuiApplication::setDesktopSettingsAware(desktopSettingsAware);
	// Before the application: the platform reads it while starting, and set after, its first connection went
	// out under the binary's name (upstream 9b0f339).
	QGuiApplication::setDesktopFileName(appId);
	// Qt's services ask the portal for a version before registering the app id, and xdg-desktop-portal 1.20+
	// refuses a registration after any other call: a WARN at every start. Qt skips both while the platform starts
	// and configure_app registers first; openUrl still goes through the portal and falls back without one.
	const bool probe = !qEnvironmentVariableIsSet("QT_NO_XDG_DESKTOP_PORTAL");
	if (probe) qputenv("QT_NO_XDG_DESKTOP_PORTAL", "1");
	std::unique_ptr<QGuiApplication> app;
	if (useQApplication) app = std::make_unique<QApplication>(argc, argv);
	else app = std::make_unique<QGuiApplication>(argc, argv);
	if (probe) qunsetenv("QT_NO_XDG_DESKTOP_PORTAL");
	return app;
}

// Upstream launch.cpp after the application exists: the portal registration, transparent
// windows, NativeTextRendering, and hidden windows never quit the shell.
inline void configure_app(const QString& appId, bool nativeTextRendering) {
	// The first portal call on this connection (see create_app); best effort, like Qt's.
	if (qEnvironmentVariableIntValue("QT_NO_XDG_DESKTOP_PORTAL") <= 0) {
		auto msg = QDBusMessage::createMethodCall(
		    "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.host.portal.Registry", "Register");
		msg.setArguments({appId, QVariantMap()});
		QDBusConnection::sessionBus().send(msg);
	}
	QQuickWindow::setDefaultAlphaBuffer(true);
	if (nativeTextRendering) QQuickWindow::setTextRenderType(QQuickWindow::NativeTextRendering);
	QGuiApplication::setQuitOnLastWindowClosed(false);
	// QSRS_QT_ANIMATION_TIMER=1 keeps Qt's 16 ms tick (A/B).
	if (!qEnvironmentVariableIsSet("QSRS_PER_WINDOW_GL") && !qEnvironmentVariableIsSet("QSRS_QT_ANIMATION_TIMER")) {
		(new DisplayAnimationDriver(qApp))->install();
	}
	if (qEnvironmentVariableIsSet("QSRS_FRAME_LOG")) FrameMeter::get();
	new PolishRelay(qApp);
}

QString memory_report(bool reclaimFirst);

// `qs frames ...`, `qs memory [reclaim]` and `qs ui ...` (sent as the frames actions "memory", "ui"): runtime
// commands answered on the GUI thread.
inline rust::String frames_command(rust::Str action, rust::Str arg) {
	if (action == rust::Str("memory")) {
		QString out;
		bool reclaimFirst = arg == rust::Str("reclaim");
		QMetaObject::invokeMethod(qApp, [&] { out = memory_report(reclaimFirst); }, Qt::BlockingQueuedConnection);
		return rust::String(out.toStdString());
	}
	if (action == rust::Str("ui")) {
		QString out;
		auto words = QString::fromUtf8(arg.data(), arg.size());
		QMetaObject::invokeMethod(qApp, [&] { out = qsrs::ui::report(words); }, Qt::BlockingQueuedConnection);
		return rust::String(out.toStdString());
	}
	auto out = frames_command_on_gui(QString::fromUtf8(action.data(), action.size()), QString::fromUtf8(arg.data(), arg.size()));
	return rust::String(out.toStdString());
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
inline QString memory_line(const char* stage) {
	long pages = 0, rss = 0;
	if (FILE* f = fopen("/proc/self/statm", "r")) { if (fscanf(f, "%ld %ld", &pages, &rss) != 2) rss = 0; fclose(f); }
	uint64_t epoch = 1;
	size_t len = sizeof epoch;
	mallctl("epoch", &epoch, &len, &epoch, len);
	auto stat = [](const char* name) { size_t v = 0, l = sizeof v; return mallctl(name, &v, &l, nullptr, 0) == 0 ? v >> 20 : 0; };
	return QString::asprintf("qsrs-memory %-10s rss=%ldMB allocated=%zuMB active=%zuMB resident=%zuMB retained=%zuMB", stage,
	                         rss * sysconf(_SC_PAGESIZE) >> 20, stat("stats.allocated"), stat("stats.active"),
	                         stat("stats.resident"), stat("stats.retained"));
}
inline void print_memory(const char* stage) { fprintf(stderr, "%s\n", qPrintable(memory_line(stage))); }

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

inline QStringList census_lines() {
	QStringList out;
	QHash<QString, int> byFile;
	QHash<QString, QPair<int, qint64>> images, layers;
	// Live texts by family and weight: what keeps a font loaded (count, hidden, first file).
	struct Face { int count = 0, hidden = 0; QString file; };
	QHash<QString, Face> faces;
	// What ticks at rest: running repeating timers (by file and interval) and running animations.
	QHash<QString, int> ticking;
	QSet<QObject*> seen;
	// owner: the QML file nearest above, so a layer's or MultiEffect's inner C++ items are weighed under the file
	// that asked for them.
	std::function<void(QObject*, const QString&)> walk = [&](QObject* o, const QString& owner) {
		if (!o || seen.contains(o)) return;
		seen.insert(o);
		auto* ctx = qmlContext(o);
		auto file = ctx ? ctx->baseUrl().fileName() : QString("<c++> ") + o->metaObject()->className();
		auto here = ctx ? file : owner;
		byFile[file]++;
		if (auto* item = qobject_cast<QQuickItem*>(o); item && item->isTextureProvider()) {
			// What the item puts on the GPU: a loaded image's decoded size (its implicit size at the dpr), or
			// the offscreen texture of a layer or a ShaderEffectSource that has a source (its textureSize,
			// else its own size). An empty Image or a sourceless one weighs nothing. Shared images count
			// once per item: an upper bound.
			bool image = QByteArray(o->metaObject()->className()).contains("Image");
			auto dpr = item->window() ? item->window()->devicePixelRatio() : 1.0;
			QSize size;
			if (image) {
				if (o->property("status").toInt() == 1) size = (QSizeF(item->implicitWidth(), item->implicitHeight()) * dpr).toSize();
			} else if (o->metaObject()->indexOfProperty("sourceItem") < 0 || o->property("sourceItem").value<QObject*>()) {
				size = o->property("textureSize").toSize();
				if (size.isEmpty()) size = (item->size() * dpr).toSize();
			}
			if (!size.isEmpty()) {
				auto& slot = (image ? images : layers)[ctx || owner.isEmpty() ? file : QString("%1 <%2>").arg(owner, o->metaObject()->className())];
				slot.first++;
				slot.second += qint64(size.width()) * size.height() * 4;
			}
		}
		if (auto* item = qobject_cast<QQuickItem*>(o); item && (o->inherits("QQuickText") || o->inherits("QQuickTextInput") || o->inherits("QQuickTextEdit"))) {
			auto font = o->property("font").value<QFont>();
			auto& face = faces[QString("%1 %2").arg(font.family()).arg(font.weight())];
			face.count++;
			if (!item->isVisible()) face.hidden++;
			if (face.file.isEmpty()) face.file = file;
		}
		if (o->inherits("QQmlTimer") && o->property("running").toBool() && o->property("repeat").toBool())
			ticking[QString("timer %1ms %2").arg(o->property("interval").toInt()).arg(here)]++;
		else if (o->inherits("QQuickAbstractAnimation") && o->property("running").toBool() && !o->property("paused").toBool())
			ticking[QString("animation %1 %2").arg(o->metaObject()->className(), here)]++;
		for (auto* c: o->children()) walk(c, here);
		if (auto* item = qobject_cast<QQuickItem*>(o)) for (auto* c: item->childItems()) walk(c, here);
	};
	QStringList windows;
	for (auto* w: QGuiApplication::allWindows()) {
		auto before = seen.size();
		walk(w, {});
		if (auto* qw = qobject_cast<QQuickWindow*>(w)) walk(qw->contentItem(), {});
		auto* ctx = qmlContext(w);
		windows << QString("%1%2:%3").arg(w->isVisible() ? "" : "-", ctx ? ctx->baseUrl().fileName() : w->metaObject()->className()).arg(seen.size() - before);
	}
	walk(QCoreApplication::instance(), {});
	QList<QPair<int, QString>> top;
	for (auto it = byFile.begin(); it != byFile.end(); ++it) top.append({it.value(), it.key()});
	std::sort(top.begin(), top.end(), [](auto& a, auto& b) { return a.first > b.first; });
	out << QString::asprintf("qsrs-census total=%lld windows=%lld", (long long)seen.size(), (long long)QGuiApplication::allWindows().size());
	out << "qsrs-census windows " + windows.join(' ');
	for (int i = 0; i < std::min<int>(40, top.size()); i++)
		out << QString::asprintf("qsrs-census %6d %s", top[i].first, qPrintable(top[i].second));
	for (auto [kind, table]: {std::pair {"images", &images}, std::pair {"layers", &layers}}) {
		QList<QPair<qint64, QString>> heavy;
		qint64 total = 0;
		for (auto it = table->begin(); it != table->end(); ++it) {
			total += it.value().second;
			heavy.append({it.value().second, QString("%1x %2").arg(it.value().first).arg(it.key())});
		}
		std::sort(heavy.begin(), heavy.end(), [](auto& a, auto& b) { return a.first > b.first; });
		out << QString::asprintf("qsrs-census %s total=%lldMB", kind, (long long)(total >> 20));
		for (int i = 0; i < std::min<int>(12, heavy.size()) && heavy[i].first >= (1 << 20); i++)
			out << QString::asprintf("qsrs-census %s %5lldMB %s", kind, (long long)(heavy[i].first >> 20), qPrintable(heavy[i].second));
	}
	out << QString::asprintf("qsrs-census ticking %lld", (long long)ticking.size());
	for (auto it = ticking.begin(); it != ticking.end(); ++it)
		out << QString::asprintf("qsrs-census ticking %4d %s", it.value(), qPrintable(it.key()));
	QList<QPair<int, QString>> byFace;
	for (auto it = faces.begin(); it != faces.end(); ++it)
		byFace.append({it->count, QString::asprintf("%5d %s (%d hidden) first in %s", it->count, qPrintable(it.key()), it->hidden, qPrintable(it->file))});
	std::sort(byFace.begin(), byFace.end(), [](auto& a, auto& b) { return a.first > b.first; });
	out << QString::asprintf("qsrs-census fonts faces=%lld", (long long)byFace.size());
	for (auto& [count, line]: byFace) out << "qsrs-census fonts " + line;
	return out;
}
inline void print_census() {
	for (auto& line: census_lines()) fprintf(stderr, "%s\n", qPrintable(line));
}

// A font file stays mapped while any text draws in it, and every outline a Design rasterized
// keeps its pages in the shell (Material Symbols: 4.5 -> 12 MB after one AIRISU round trip).
// Those glyphs sit in the glyph caches by then, so the pages of the read-only private font
// mappings go back to the page cache; a new glyph faults its outline in again, from memory.
inline void release_font_pages() {
	FILE* f = fopen("/proc/self/maps", "r");
	if (!f) return;
	char line[4096];
	while (fgets(line, sizeof line, f)) {
		unsigned long start = 0, end = 0;
		char perms[8] = {};
		int path = 0;
		if (sscanf(line, "%lx-%lx %7s %*s %*s %*s %n", &start, &end, perms, &path) < 3 || path == 0) continue;
		if (strcmp(perms, "r--p") != 0) continue;
		QByteArray file = QByteArray(line + path).trimmed();
		static const char* fonts[] = {".ttf", ".otf", ".ttc", ".otc", ".pfb", ".woff", ".woff2"};
		if (std::none_of(std::begin(fonts), std::end(fonts), [&](const char* ext) { return file.endsWith(ext); })) continue;
		madvise(reinterpret_cast<void*>(start), end - start, MADV_DONTNEED);
	}
	fclose(f);
}

// quickshell/src/cpp/windowprivate.cpp: frees the glyph caches and fonts no text of a shown
// window uses (declared here: window.h needs QtGui's semi-public include dir).
void releaseUnusedGlyphCaches(QQuickWindow* window);

// Memory goes back after surfaces close. Freed QML objects leave garbage in the JS heap until an
// allocation-driven GC, and jemalloc keeps the freed pages dirty (its decay did not return them
// while the shell idles): measured after opening and closing every inir panel, RSS stayed 160 MB
// over start with the object count back at its start value. A hidden window starts two passes:
// one at 6 s (the surface is gone) and one at 26 s (inir unloads closed panels ~20 s later).
// Each pass: JS GC, unused compiled components, the scene graph's caches of hidden windows,
// then a jemalloc purge. Then what only a torn-down tree drew: glyph caches and font engines no
// text uses (their font files unmap), and the pages of the fonts still in use go back to the
// page cache (one AIRISU round trip kept ~36 MB of this). QSRS_NO_RECLAIM=1 turns all of it
// off, QSRS_NO_FONT_RECLAIM=1 only the font part (A/B).
inline void reclaim(QQmlEngine* engine, bool hiddenScenes = true) {
	engine->collectGarbage();
	engine->trimComponentCache();
	for (auto* w: QGuiApplication::allWindows())
		if (auto* qw = qobject_cast<QQuickWindow*>(w); hiddenScenes && qw && !qw->isVisible()) qw->releaseResources();
	mallctl("arena.4096.purge", nullptr, nullptr, nullptr, 0);
	if (qEnvironmentVariableIntValue("QSRS_NO_FONT_RECLAIM") == 1) return;
	QFont::cleanup();
	for (auto* w: QGuiApplication::allWindows())
		if (auto* qw = qobject_cast<QQuickWindow*>(w)) releaseUnusedGlyphCaches(qw);
	// The caches go on each render thread's next sync; their pages come back after it.
	QTimer::singleShot(1000, qApp, [] {
		release_font_pages();
		mallctl("arena.4096.purge", nullptr, nullptr, nullptr, 0);
	});
}

// Watches window Hide events and large teardowns (no polling: an idle shell is never woken for this).
// A teardown is what a visible surface does without hiding: a Design switch swaps every skin, a
// Settings page or a Loader drops its tree. Qt's object-removal hook (the one debuggers use,
// chained) counts destroyed objects from any thread; once a burst passes `burst` objects inside
// `window` ms, the watcher is told once and reclaims when the teardown has been quiet for
// `settle` ms. Measured on inir: each iRiS <-> AIRISU switch kept 7-34 MB until a window hid.
class ReclaimWatcher: public QObject {
public:
	static constexpr int burst = 2000;
	static constexpr qint64 window = 10000;
	static constexpr int settle = 3000;

	// burstOnly: only after a large teardown, and hidden windows keep their scenes (what made panels lose a frame).
	explicit ReclaimWatcher(QQmlEngine* engine, bool burstOnly = false): QObject(engine), engine(engine), burstOnly(burstOnly) {
		for (auto* t: {&this->early, &this->late, &this->quiet}) {
			t->setSingleShot(true);
			QObject::connect(t, &QTimer::timeout, this, [this] { this->run(); });
		}
		this->early.setInterval(6000);
		// burstOnly: a second pass 40 s after a teardown, for what inir lets go a while later (a family it left).
		this->late.setInterval(burstOnly ? 40000 : 26000);
		this->quiet.setInterval(settle);
		if (!burstOnly) qApp->installEventFilter(this);
		if (previousRemove == nullptr && qtHookData[removeHook] != reinterpret_cast<quintptr>(&removed)) {
			previousRemove = reinterpret_cast<RemoveCallback>(qtHookData[removeHook]);
			qtHookData[removeHook] = reinterpret_cast<quintptr>(&removed);
		}
		current.store(this);
	}
	~ReclaimWatcher() override {
		auto* self = this;
		current.compare_exchange_strong(self, nullptr);
	}

	static QQmlEngine* engineNow() {
		auto* watcher = current.load();
		return watcher ? watcher->engine : nullptr;
	}

	// Objects destroyed in the burst being counted (QSRS_RECLAIM_LOG=1 prints each reclaim).
	static inline std::atomic<int> destroyed {0};

protected:
	bool eventFilter(QObject* o, QEvent* e) override {
		if (e->type() == QEvent::Hide && o->isWindowType()) {
			this->early.start();
			this->late.start();
		}
		return false;
	}

private:
	static inline std::atomic<ReclaimWatcher*> current {nullptr};
	static inline std::atomic<bool> told {false};
	static inline std::atomic<qint64> burstStart {0};
	using RemoveCallback = void (*)(QObject*);
	static constexpr int removeHook = 4;
	static inline RemoveCallback previousRemove = nullptr;

	static qint64 nowMs() {
		return std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
	}

	// Runs inside every QObject destructor, on any thread: a counter and, once per burst, one
	// queued call to the watcher.
	static void removed(QObject* o) {
		if (previousRemove) previousRemove(o);
		auto now = nowMs();
		auto start = burstStart.load(std::memory_order_relaxed);
		if (now - start > window) {
			burstStart.store(now, std::memory_order_relaxed);
			destroyed.store(0, std::memory_order_relaxed);
		}
		if (destroyed.fetch_add(1, std::memory_order_relaxed) + 1 < burst) return;
		auto* watcher = current.load();
		if (watcher == nullptr || told.exchange(true)) return;
		QMetaObject::invokeMethod(watcher, [watcher] {
			watcher->lastCount = destroyed.load(std::memory_order_relaxed);
			watcher->quiet.start();
		}, Qt::QueuedConnection);
	}

	void run() {
		// A teardown still going (its count grew since the burst was told) waits another round.
		if (sender() == &this->quiet) {
			int count = destroyed.load(std::memory_order_relaxed);
			if (count > this->lastCount + burst / 4) {
				this->lastCount = count;
				this->quiet.start();
				return;
			}
		}
		static const bool log = qEnvironmentVariableIntValue("QSRS_RECLAIM_LOG") == 1;
		if (log) print_memory(sender() == &this->quiet ? "burst" : "hidden");
		reclaim(this->engine, !this->burstOnly);
		if (log) print_memory("reclaimed");
		if (sender() == &this->quiet) {
			if (this->burstOnly) this->late.start();
			told.store(false);
			destroyed.store(0, std::memory_order_relaxed);
		}
	}

	QQmlEngine* engine;
	bool burstOnly;
	QTimer early;
	QTimer late;
	QTimer quiet;
	int lastCount = 0;
};

// Where the memory peak happened (AU-F4: 890 MB once against 365 at rest, and the journal had
// rotated it away). The kernel keeps the process maximum (VmHWM); it is read for 10 s after
// each window shows or hides and each reload, and every 30 s otherwise. Each time it rises by
// 32 MB the shell logs it with the visible windows and the last events, and appends the line
// to $XDG_STATE_HOME/qs-rs/memory-peaks (kept under 64 KB). The startup peak is one line.
class PeakWatcher: public QObject {
public:
	static PeakWatcher* instance;

	PeakWatcher(): QObject(QCoreApplication::instance()) {
		this->started.start();
		this->burst.setInterval(250);
		QObject::connect(&this->burst, &QTimer::timeout, this, [this] {
			this->sample();
			if (--this->burstLeft <= 0) this->burst.stop();
		});
		this->slow.setInterval(30000);
		QObject::connect(&this->slow, &QTimer::timeout, this, [this] { this->sample(); });
		this->slow.start();
		// precise: a coarse timer may fire 5 % early, before the 5 s a sample needs, and the startup line waits 30 s
		QTimer::singleShot(5100, Qt::PreciseTimer, this, [this] { this->sample(); });
		qApp->installEventFilter(this);
	}

	// The file that declares the window's Quickshell object (the backing QQuickWindow has no
	// QML context of its own); a window frozen across a reload says so.
	static QString name(QObject* window) {
		auto* proxy = window->property("_qsWindow").value<QObject*>();
		auto* ctx = qmlContext(proxy ? proxy : window);
		auto out = ctx ? ctx->baseUrl().fileName() : QString(window->metaObject()->className());
		if (proxy && !proxy->objectName().isEmpty()) out += "#" + proxy->objectName();
		if (window->property("_qsFrozen").toBool()) out += " (frozen)";
		return out;
	}

	void note(const QString& event) {
		this->events.append({this->started.elapsed(), event});
		if (this->events.size() > 8) this->events.removeFirst();
		this->burstLeft = 40;
		if (!this->burst.isActive()) this->burst.start();
	}

protected:
	bool eventFilter(QObject* o, QEvent* e) override {
		if ((e->type() == QEvent::Show || e->type() == QEvent::Hide) && o->isWindowType()) {
			this->note(QString(e->type() == QEvent::Show ? "show " : "hide ") + name(o));
		}
		return false;
	}

private:
	static void read(long& rssMb, long& hwmMb) {
		rssMb = hwmMb = 0;
		FILE* f = fopen("/proc/self/status", "r");
		if (!f) return;
		char line[128];
		while (fgets(line, sizeof line, f)) {
			if (!strncmp(line, "VmHWM:", 6)) hwmMb = atol(line + 6) >> 10;
			else if (!strncmp(line, "VmRSS:", 6)) rssMb = atol(line + 6) >> 10;
		}
		fclose(f);
	}

	void sample() {
		long rss = 0, hwm = 0;
		read(rss, hwm);
		bool settled = this->started.elapsed() > 5000;
		if (!settled) return;
		if (this->logged == 0) {
			this->logged = hwm;
			this->report(QString("startup peak %1 MB, now %2 MB").arg(hwm).arg(rss));
			return;
		}
		if (hwm < this->logged + 32) return;
		this->logged = hwm;
		this->report(QString("new peak %1 MB, now %2 MB").arg(hwm).arg(rss));
	}

	void report(const QString& head) {
		QStringList visible;
		for (auto* w: QGuiApplication::allWindows()) {
			if (w->isVisible()) visible << name(w);
		}
		visible.sort();
		QStringList recent;
		auto now = this->started.elapsed();
		for (auto& [at, what]: this->events) recent << QString("%1 (%2 s ago)").arg(what).arg((now - at) / 1000.0, 0, 'f', 1);
		auto line = QString("Memory: %1; visible: %2; last events: %3")
		                .arg(head, visible.isEmpty() ? "none" : visible.join(", "), recent.isEmpty() ? "none" : recent.join(", "));
		qInfo().noquote() << line;
		auto dir = qEnvironmentVariable("XDG_STATE_HOME", QDir::homePath() + "/.local/state") + "/qs-rs";
		QDir().mkpath(dir);
		QFile file(dir + "/memory-peaks");
		if (file.size() > 64 * 1024 && file.open(QIODevice::ReadOnly)) {
			auto keep = file.readAll().right(32 * 1024);
			file.close();
			if (file.open(QIODevice::WriteOnly | QIODevice::Truncate)) file.write(keep.mid(keep.indexOf('\n') + 1));
			file.close();
		}
		if (file.open(QIODevice::Append)) {
			file.write((QDateTime::currentDateTime().toString(Qt::ISODate) + " pid " + QString::number(getpid()) + " " + line + "\n").toUtf8());
		}
	}

	QElapsedTimer started;
	QTimer burst;
	QTimer slow;
	int burstLeft = 0;
	long logged = 0;
	QList<QPair<qint64, QString>> events;
};
inline PeakWatcher* PeakWatcher::instance = nullptr;

// `qs memory`: where the running shell's memory sits (process and jemalloc), its live objects by
// file and what its images and offscreen layers hold. `reclaim` runs a reclaim pass first.
inline QString memory_report(bool reclaimFirst) {
	QStringList out;
	if (reclaimFirst) {
		auto* engine = ReclaimWatcher::engineNow();
		out << memory_line("before");
		if (engine) reclaim(engine);
		out << memory_line(engine ? "reclaimed" : "no-engine");
	} else {
		out << memory_line("now");
	}
	out << census_lines();
	return out.join('\n');
}

inline void watch_memory_peaks() {
	if (PeakWatcher::instance == nullptr) PeakWatcher::instance = new PeakWatcher();
}

// After a large teardown (a family switch, a closed Settings let go) the dead tree's compiled components, JS heap and
// pages go back: without it two rounds of inir's three families kept +57 MB with the object count back at its start.
// QSRS_RECLAIM=1 also reclaims when a window hides, dropping hidden windows' scenes: that made every panel, tray menu
// and morph rebuild on its next show (a lost first frame, measured on the Control Center). QSRS_NO_RECLAIM=1: none.
inline void schedule_reclaim(QQmlEngine* engine) {
	if (qEnvironmentVariableIntValue("QSRS_NO_RECLAIM") == 1) return;
	new ReclaimWatcher(engine, qEnvironmentVariableIntValue("QSRS_RECLAIM") != 1);
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
	// Registered until the QObject goes, after the V4 engine: a crash while it is torn down
	// still names the JS that ran, and a read past its end is caught by the handler.
	qsrs::crash_engine_add(engine->handle());
	QObject::connect(engine, &QObject::destroyed, [v4 = engine->handle()] { qsrs::crash_engine_remove(v4); });
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

// A generation where a component failed to load (a type unavailable, a property set twice) left its windows
// half built: reused by the next reload they kept a body with no content, no menus and no motion until a restart.
// Their windows are not carried over; the next generation builds its own. Loaders report these through the logger,
// not QQmlEngine::warnings, as a warning that names the file and line it failed at. Info and debug lines are never
// one (inir's Weather logs "GPS unavailable" at info, and QtInfoMsg sorts above QtWarningMsg), nor is a script's
// console.warn, whose text carries no location.
struct LoadWatch {
	static inline std::atomic<bool> broken {false};
	static bool failedLoad(QtMsgType type, const QString& msg) {
		if (type != QtWarningMsg && type != QtCriticalMsg) return false;
		if (!msg.contains(QLatin1String(".qml:"))) return false;
		return msg.contains(QLatin1String(" unavailable")) || msg.contains(QLatin1String("set multiple times"))
		    || msg.contains(QLatin1String("is not a type")) || msg.contains(QLatin1String("is not installed"));
	}
	// Before a generation is created, the first one included: what its creation logs counts against it.
	static void begin() {
		broken.store(false);
		static QtMessageHandler chained = nullptr;
		static bool installed = false;
		if (installed) return;
		installed = true;
		chained = qInstallMessageHandler([](QtMsgType type, const QMessageLogContext& context, const QString& msg) {
			if (failedLoad(type, msg)) broken.store(true);
			if (chained) chained(type, context, msg);
		});
	}
};

// Creates the root object from a local file; prints errors and returns null on failure.
inline QObject* create_root(QQmlEngine& engine, const QString& path) {
	QQmlComponent component(&engine, QUrl::fromLocalFile(path));
	// Quickshell's own line: setup's update check reads it to know the new version did not load
	auto failed = [&] {
		qCritical() << "Failed to load configuration";
		for (auto& e: component.errors()) qCritical().noquote() << e.toString();
		return nullptr;
	};
	if (component.isError()) return failed();
	LoadWatch::begin();
	auto* root = component.create();
	if (root == nullptr) return failed();
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
		if (PeakWatcher::instance != nullptr) PeakWatcher::instance->note("reload");
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
		if (LoadWatch::broken.load()) qInfo() << "Previous generation failed to load parts: its windows are rebuilt";
		else QsWindowPool::stash();
		// The old generation: root first (its children may still reach the engine), then the engine.
		// No JS runs while it comes apart (upstream EngineGeneration::destroy): the engine marks
		// its singletons deleted before deleting them, but not the parentless JS objects they
		// hold, whose bindings would then read singletons already freed (SEGV in
		// lookupSingletonProperty, tests/qml/reloadteardown.qml).
		// QSRS_TEST_TEARDOWN_JS keeps the old bug, for tests/crashstack to crash mid-reload.
		static const bool keepJs = qEnvironmentVariableIsSet("QSRS_TEST_TEARDOWN_JS");
		if (!keepJs) this->engine->setInterrupted(true);
		// With JS interrupted, every binding the teardown wakes evaluates to undefined and Qt warns
		// "Unable to assign [undefined]" for each (hundreds per reload in inir): noise about objects
		// already on their way out. Only that line is dropped, and only while the old engine dies.
		static QtMessageHandler forward = nullptr;
		forward = qInstallMessageHandler([](QtMsgType type, const QMessageLogContext& context, const QString& msg) {
			if (type == QtWarningMsg && msg.contains(QLatin1String("Unable to assign [undefined]"))) return;
			if (forward) forward(type, context, msg);
		});
		delete this->root;
		this->root = nullptr;
		QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
		this->engine->collectGarbage();
		delete this->engine;
		qInstallMessageHandler(forward);
		this->engine = next;
		LoadWatch::begin();

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
