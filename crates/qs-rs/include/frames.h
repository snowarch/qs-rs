#pragma once
// The animation driver and the frame meter. Both follow the frames each window really draws,
// which the basic render loop (one GL context, everything on the GUI thread) runs on each
// UpdateRequest: polish, afterAnimating, beginFrame, sync, render, endFrame (swap),
// frameSwapped. Order read from Qt 6.12's QSGGuiThreadRenderLoop::renderWindow.
#include <QtCore/QAbstractAnimation>
#include <QtCore/QChronoTimer>
#include <QtCore/QFile>
#include <QtCore/QHash>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QPointer>
#include <QtCore/QThread>
#include <QtCore/QTimer>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtQml/QQmlContext>
#include <QtQuick/QQuickWindow>
#include <algorithm>
#include <cmath>
#include <vector>
#include "quickshell/include/frameclock.h"

namespace qsrs {
namespace fc = frameclock;

// The fastest screen's refresh period, which the driver steps at. Tests have no real screen:
// QSRS_ANIMATION_HZ stands in for one.
inline qint64 driver_period_ns() {
	qreal hz = 60;
	for (auto* s: QGuiApplication::screens()) hz = std::max(hz, s->refreshRate());
	if (auto forced = qEnvironmentVariableIntValue("QSRS_ANIMATION_HZ"); forced > 0) hz = forced;
	return qint64(1e9 / hz);
}

inline qint64 window_period_ns(QWindow* w) {
	if (auto forced = qEnvironmentVariableIntValue("QSRS_ANIMATION_HZ"); forced > 0) return qint64(1e9 / forced);
	auto* s = w->screen();
	qreal hz = s && s->refreshRate() > 1 ? s->refreshRate() : 60;
	return qint64(1e9 / hz);
}

// Per window and per frame: where the time went, and how many refreshes went by without a new
// frame while the window was asking for one. `qs frames start|stop|stats|reset` or
// QSRS_FRAME_LOG=FILE at launch; FILE gets one line per frame (frames.h at the end of this file
// documents the fields). Off by default: no hooks, no cost.
class FrameMeter: public QObject {
public:
	static FrameMeter* get() {
		static auto* meter = new FrameMeter();
		return meter;
	}
	bool active() const { return this->on; }

	// The driver's steps between frames (each one runs every animation's bindings and JS).
	void noteTick(qint64 start, qint64 end) {
		if (!this->on) return;
		this->ticks++;
		this->tickNs += end - start;
		for (auto it = this->wins.begin(); it != this->wins.end(); ++it) {
			auto& w = it.value();
			w.ticks++;
			w.tickNs += end - start;
			// Wanted from the step's start: a step with slow JS is the window's frame running late.
			if (!w.requestAt && fc::update_pending(it.key())) w.requestAt = start;
		}
	}

	QString command(const QString& action, const QString& arg) {
		if (action == "start") {
			this->start(arg);
			return QString("frame meter on%1").arg(this->log ? ", writing " + this->log->fileName() : QString());
		}
		if (action == "stop") {
			auto out = this->stats();
			this->stop();
			return out;
		}
		if (action == "stats") return this->stats();
		if (action == "reset") {
			this->reset();
			return "reset";
		}
		return "unknown action (start [FILE], stop, stats, reset)";
	}

	void start(const QString& path) {
		this->stop();
		if (!path.isEmpty() && path != "-") {
			this->log = new QFile(path, this);
			if (!this->log->open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) {
				qWarning().noquote() << "frame meter: cannot write" << path;
				delete this->log;
				this->log = nullptr;
			}
		}
		this->on = true;
		this->reset();
		qApp->installEventFilter(this);
		this->flush.start();
	}

	void stop() {
		if (!this->on) return;
		this->on = false;
		qApp->removeEventFilter(this);
		for (auto& w: this->wins)
			for (auto& c: w.conns) QObject::disconnect(c);
		this->wins.clear();
		this->flush.stop();
		if (this->log) {
			this->log->close();
			delete this->log;
			this->log = nullptr;
		}
	}

	void reset() {
		this->since = fc::now_ns();
		this->ticks = 0;
		this->tickNs = 0;
		for (auto& w: this->wins) w.stats = {};
	}

protected:
	bool eventFilter(QObject* o, QEvent* e) override {
		if (e->type() != QEvent::UpdateRequest || !o->isWindowType()) return false;
		auto* qw = qobject_cast<QQuickWindow*>(o);
		if (!qw) return false;
		auto& w = this->hook(qw);
		w.start = fc::now_ns();
		w.animated = w.synced = w.rendered = 0;
		return false;
	}

private:
	struct Stats {
		qint64 frames = 0, continuous = 0, missed = 0, continuousNs = 0, ticks = 0, tickNs = 0;
		qint64 polishNs = 0, syncNs = 0, renderNs = 0, swapNs = 0;
		std::vector<float> costs; // ms, the last 8192 frames
	};
	struct Win {
		QList<QMetaObject::Connection> conns;
		qint64 start = 0, animated = 0, synced = 0, rendered = 0, lastSwap = 0, requestAt = 0;
		qint64 ticks = 0, tickNs = 0;
		Stats stats;
		QString name;
	};

	FrameMeter(): QObject(qApp) {
		this->flush.setInterval(200);
		QObject::connect(&this->flush, &QTimer::timeout, this, [this] {
			if (this->log) this->log->flush();
		});
		if (auto path = qEnvironmentVariable("QSRS_FRAME_LOG"); !path.isEmpty()) this->start(path == "1" ? "-" : path);
	}

	// A layer surface by its namespace, any other window by the file that declares it.
	static QString nameOf(QQuickWindow* w) {
		auto ns = w->property("_qsNamespace").toString();
		if (!ns.isEmpty()) return ns;
		auto* proxy = w->property("_qsWindow").value<QObject*>();
		auto* ctx = qmlContext(proxy ? proxy : w);
		auto out = ctx ? ctx->baseUrl().fileName() : (w->title().isEmpty() ? QString(w->metaObject()->className()) : w->title());
		return out.replace(' ', '_');
	}

	Win& hook(QQuickWindow* qw) {
		auto it = this->wins.find(qw);
		if (it != this->wins.end()) return it.value();
		auto& w = this->wins[qw];
		auto at = [this, qw](qint64 Win::* field) {
			return [this, qw, field] {
				auto it = this->wins.find(qw);
				if (it != this->wins.end()) it.value().*field = fc::now_ns();
			};
		};
		w.conns << QObject::connect(qw, &QQuickWindow::afterAnimating, this, at(&Win::animated), Qt::DirectConnection);
		w.conns << QObject::connect(qw, &QQuickWindow::afterSynchronizing, this, at(&Win::synced), Qt::DirectConnection);
		w.conns << QObject::connect(qw, &QQuickWindow::afterRendering, this, at(&Win::rendered), Qt::DirectConnection);
		w.conns << QObject::connect(qw, &QQuickWindow::frameSwapped, this, [this, qw] { this->swapped(qw); }, Qt::DirectConnection);
		w.conns << QObject::connect(qw, &QObject::destroyed, this, [this, qw] { this->wins.remove(qw); });
		return w;
	}

	void swapped(QQuickWindow* qw) {
		auto it = this->wins.find(qw);
		if (it == this->wins.end()) return;
		auto& w = it.value();
		auto now = fc::now_ns();
		if (!w.start || !w.animated || !w.synced || !w.rendered) {
			w.start = 0;
			return;
		}
		auto period = window_period_ns(qw);
		auto delta = w.lastSwap ? now - w.lastSwap : 0;
		// Asked for this frame by the time the last one was out (the next step or a post-frame
		// update): every refresh the interval spans past the first went by without a frame.
		bool continuous = w.lastSwap && w.requestAt && w.requestAt <= w.lastSwap + period / 4;
		qint64 missed = (now - w.start) / period;
		// A frame later than 1.2 refreshes after the last one is on screen a vblank late at least
		// (the swap is not the vblank: a compositor without one can take a frame 25 ms in at 60 Hz).
		if (continuous) missed = std::max<qint64>(missed, qint64(std::ceil(double(delta) / period - 1.2)));
		missed = std::max<qint64>(0, missed);

		auto& s = w.stats;
		s.frames++;
		s.missed += missed;
		if (continuous) {
			s.continuous++;
			s.continuousNs += delta;
		}
		s.ticks += w.ticks;
		s.tickNs += w.tickNs;
		s.polishNs += w.animated - w.start;
		s.syncNs += w.synced - w.animated;
		s.renderNs += w.rendered - w.synced;
		s.swapNs += now - w.rendered;
		float cost = (now - w.start) / 1e6f;
		if (s.costs.size() >= 8192) s.costs.erase(s.costs.begin(), s.costs.begin() + 4096);
		s.costs.push_back(cost);
		w.name = nameOf(qw);

		if (this->log) {
			auto ms = [](qint64 ns) { return QString::number(ns / 1e6, 'f', 3); };
			auto line = QString("t=%1 win=%2 d=%3 miss=%4 cont=%5 ticks=%6 tick=%7 polish=%8 sync=%9 render=%10 swap=%11 cost=%12 hz=%13\n")
				.arg(now / 1000).arg(w.name, ms(delta)).arg(missed).arg(continuous ? 1 : 0).arg(w.ticks)
				.arg(ms(w.tickNs), ms(w.animated - w.start), ms(w.synced - w.animated), ms(w.rendered - w.synced), ms(now - w.rendered), ms(now - w.start))
				.arg(QString::number(1e9 / period, 'f', 2));
			this->log->write(line.toUtf8());
		}
		w.lastSwap = now;
		w.start = 0;
		w.ticks = 0;
		w.tickNs = 0;
		// A request made while this frame rendered (the window is still animating).
		w.requestAt = fc::update_pending(qw) ? now : 0;
	}

	QString stats() const {
		auto seconds = (fc::now_ns() - this->since) / 1e9;
		QJsonArray list;
		for (auto it = this->wins.begin(); it != this->wins.end(); ++it) {
			const auto& s = it.value().stats;
			if (!s.frames) continue;
			auto sorted = s.costs;
			std::sort(sorted.begin(), sorted.end());
			auto pct = [&](double p) { return sorted.empty() ? 0.0 : double(sorted[std::min(sorted.size() - 1, size_t(p * sorted.size()))]); };
			auto mean = [&](qint64 ns) { return std::round(ns / 1e3 / s.frames) / 1e3; };
			auto period = window_period_ns(it.key());
			list.append(QJsonObject {
				{"win", it.value().name},
				{"hz", std::round(1e11 / period) / 100},
				{"frames", s.frames},
				{"missed", s.missed},
				{"continuous", s.continuous},
				// Frames a second while the window kept asking for frames: the refresh rate when
				// nothing was missed.
				{"fps", s.continuousNs ? std::round(s.continuous * 1e10 / s.continuousNs) / 10 : 0.0},
				{"cost_p50", pct(0.5)},
				{"cost_p95", pct(0.95)},
				{"cost_p99", pct(0.99)},
				{"cost_max", sorted.empty() ? 0.0 : double(sorted.back())},
				{"polish", mean(s.polishNs)},
				{"sync", mean(s.syncNs)},
				{"render", mean(s.renderNs)},
				{"swap", mean(s.swapNs)},
				{"ticks_per_frame", std::round(s.ticks * 100.0 / s.frames) / 100},
			});
		}
		QJsonObject out {
			{"seconds", std::round(seconds * 100) / 100},
			{"ticks", this->ticks},
			{"ticks_per_s", seconds > 0 ? std::round(this->ticks / seconds * 10) / 10 : 0.0},
			{"tick_ms_mean", this->ticks ? std::round(this->tickNs / 1e3 / this->ticks) / 1e3 : 0.0},
			{"windows", list},
		};
		return QString::fromUtf8(QJsonDocument(out).toJson(QJsonDocument::Compact));
	}

	bool on = false;
	QFile* log = nullptr;
	QTimer flush;
	QHash<QQuickWindow*, Win> wins;
	qint64 since = 0, ticks = 0, tickNs = 0;
};

// The basic render loop installs no animation driver, so QML animations would step on
// QUnifiedTimer's fixed 16 ms tick: ~62 steps a second, which judders on a 75 or 144 Hz screen.
// This driver steps once per frame: right after a window's frame is out, it advances every
// animation to the time the next frame is presented (the frame's start, paced by the
// compositor's frame callback, plus one refresh), so the windows they dirty ask for that frame
// and draw it with values of its own time. A timer at 1.5 refreshes only steps when no window is
// drawing (an animation starting from rest, or one nobody shows). The old free-running timer
// plus a step at each frame's start ran every animation's JS twice a frame (motion, 10-20: 110
// steps a second on a 60 Hz nest).
class DisplayAnimationDriver: public QAnimationDriver {
public:
	explicit DisplayAnimationDriver(QObject* parent): QAnimationDriver(parent) {
		this->timer.setTimerType(Qt::PreciseTimer);
		this->timer.setSingleShot(true);
		QObject::connect(&this->timer, &QChronoTimer::timeout, this, [this] {
			if (this->isRunning()) this->step(fc::now_ns(), false);
		});
		auto watch = [this](QScreen* s) {
			QObject::connect(s, &QScreen::refreshRateChanged, this, [this] { fc::period_ns = driver_period_ns(); });
			fc::period_ns = driver_period_ns();
		};
		for (auto* s: QGuiApplication::screens()) watch(s);
		QObject::connect(qApp, &QGuiApplication::screenAdded, this, watch);
		QObject::connect(qApp, &QGuiApplication::screenRemoved, this, [] { fc::period_ns = driver_period_ns(); });
		qApp->installEventFilter(this);
	}

	bool eventFilter(QObject* watched, QEvent* event) override {
		if (event->type() == QEvent::UpdateRequest && watched->isWindowType()) {
			if (auto* w = qobject_cast<QQuickWindow*>(watched)) {
				this->hook(w);
				auto& f = this->frames[w];
				auto now = fc::now_ns();
				f.interval = f.start ? now - f.start : 0;
				f.start = now;
			}
		}
		return false;
	}

	// Qt reads it when an animation starts or a step runs: the step's time, never behind the
	// wall clock (a step for the next vblank is ahead of it).
	qint64 elapsed() const override {
		if (!this->isRunning()) return 0;
		return (std::max(fc::step_ns, fc::now_ns()) - this->origin) / 1000000;
	}

protected:
	void start() override {
		this->origin = fc::now_ns();
		fc::step_ns = this->origin;
		QAnimationDriver::start();
		this->rearm(fc::period_ns);
	}
	void stop() override {
		this->timer.stop();
		QAnimationDriver::stop();
	}

private:
	void hook(QQuickWindow* w) {
		if (this->frames.contains(w)) return;
		this->frames[w] = {};
		QObject::connect(w, &QQuickWindow::frameSwapped, this, [this, w] { this->swapped(w); }, Qt::DirectConnection);
		QObject::connect(w, &QObject::destroyed, this, [this, w] { this->frames.remove(w); });
	}

	void swapped(QQuickWindow* w) {
		if (!this->isRunning() || this->queued) return;
		auto f = this->frames.value(w);
		// The next frame starts one refresh after this one: the screen's, or the pace the
		// compositor really calls back at when that is faster (a refresh rate reported low, a
		// test without vblank). A late frame says nothing about the next one.
		auto period = window_period_ns(w);
		// The compositor calling back slower than the screen refreshes (a nested one with two
		// windows animating halves it) would slow every animation and every QML Timer with it:
		// they run on this driver. The timer then keeps the refresh between frames.
		this->slowFrames = f.interval > period * 3 / 2;
		if (f.interval > 0 && f.interval < period) period = f.interval;
		auto target = std::max(f.start ? f.start + period : 0, fc::now_ns());
		// Another window's frame of the same refresh already stepped to this vblank.
		if (target <= fc::step_ns + period / 2) return;
		this->queued = true;
		// After renderWindow returns: the step dirties windows, and a request made inside the
		// frame would be dropped by the render loop's own bookkeeping.
		QMetaObject::invokeMethod(this, [this, target] {
			this->queued = false;
			if (this->isRunning()) this->step(target);
		}, Qt::QueuedConnection);
	}

	// After a frame's step the timer waits 1.5 refreshes, so it only fires when the frames stop;
	// stepping on its own it keeps the refresh rate.
	void step(qint64 target, bool fromFrame = true) {
		auto begin = fc::now_ns();
		fc::step_ns = std::max(target, fc::step_ns);
		this->advance();
		auto end = fc::now_ns();
		FrameMeter::get()->noteTick(begin, end);
		if (this->isRunning()) this->rearm(fromFrame && !this->slowFrames ? fc::period_ns * 3 / 2 : fc::period_ns);
	}

	void rearm(qint64 ns) {
		this->timer.setInterval(std::chrono::nanoseconds(std::max<qint64>(ns, 1000000)));
		this->timer.start();
	}

	QChronoTimer timer;
	struct Frame {
		qint64 start = 0, interval = 0;
	};
	QHash<QQuickWindow*, Frame> frames;
	qint64 origin = 0;
	bool queued = false;
	bool slowFrames = false;
};

// `qs frames ...` arrives on an IPC connection thread.
inline QString frames_command_on_gui(const QString& action, const QString& arg) {
	QString out;
	if (!qApp) return "not ready";
	if (QThread::currentThread() == qApp->thread()) return FrameMeter::get()->command(action, arg);
	QMetaObject::invokeMethod(qApp, [&] { out = FrameMeter::get()->command(action, arg); }, Qt::BlockingQueuedConnection);
	return out;
}
} // namespace qsrs

// QSRS_FRAME_LOG / `qs frames start FILE`, one line per frame and window:
//   t       CLOCK_MONOTONIC µs at frameSwapped (after endFrame, the swap)
//   win     layer namespace, or the file declaring the window
//   d       ms since this window's previous frame
//   miss    refreshes that went by without a frame while the window wanted one: from the frame's
//           own cost (start to swap over the period), and on a continuous run from the interval
//           (ceil(d / period - 1.2): 25 ms at 60 Hz is one)
//   cont    1 if the window had asked for this frame by the time the previous one was out
//   ticks   animation driver steps since this window's previous frame, tick = their ms
//   polish  UpdateRequest to afterAnimating (frame-synchronous events and polish: relayout)
//   sync    afterAnimating to afterSynchronizing (beginFrame and the scene graph sync)
//   render  afterSynchronizing to afterRendering
//   swap    afterRendering to frameSwapped (endFrame, the present)
//   cost    the whole frame; hz the refresh of the window's screen
