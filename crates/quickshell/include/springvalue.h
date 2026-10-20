#pragma once
// SpringValue: IrisSpring (inir modules/iris/components/IrisSpring.qml) in native code, stepped
// by the animation driver at the time each frame is meant to be on screen.
//
// The model is Apple's Spring(duration:bounce:): mass 1, stiffness (2π/d)², damping 4π(1-bounce)/d,
// that is ω = 2π/d and ζ = 1 - bounce, integrated in closed form (as Motion's spring generator
// does) so the step size never changes the path. Qt's SpringAnimation takes fixed 16 ms Euler
// steps and react-spring 1 ms ones; both drift with the frame rate. Qt's FrameAnimation and
// IrisSpring time a step with the wall clock when the tick runs, so a late tick shows a frame
// that is early or late by however long the main thread was busy. Here dt is the distance
// between two presented frames (qsrs::frameclock), so motion is exact at any rate and under load.
//
// What it keeps from IrisSpring, on purpose: the first step is at most one frame from the kick, a
// step is at most 34 ms, a retarget keeps the velocity, a spring whose parameters carry a cubic
// curve moves as additive bezier segments (each retarget adds one), and rest is |x| < epsilon
// with |v| < 12 epsilon. One change: emerge or recede is picked once per kick (to >= value), not
// every tick, so an overshoot does not switch to the other spring halfway through a movement.
#include <QtCore/QAbstractAnimation>
#include <QtCore/QList>
#include <QtCore/QObject>
#include <QtCore/QVariant>
#include <QtQml/QJSValue>
#include <QtQml/QQmlParserStatus>
#include <QtQml/qqml.h>
#include <cmath>
#include "frameclock.h"

class SpringValue;

class QsSpringTicker: public QAbstractAnimation {
public:
	explicit QsSpringTicker(SpringValue* spring);
	[[nodiscard]] int duration() const override { return -1; }

protected:
	void updateCurrentTime(int) override;

private:
	SpringValue* spring;
};

class SpringValue: public QObject, public QQmlParserStatus {
	Q_OBJECT;
	Q_INTERFACES(QQmlParserStatus);
	Q_PROPERTY(qreal to READ to WRITE setTo NOTIFY toChanged);
	Q_PROPERTY(qreal value READ value NOTIFY valueChanged);
	Q_PROPERTY(qreal velocity READ velocity WRITE setVelocity NOTIFY velocityChanged);
	Q_PROPERTY(bool running READ isRunning NOTIFY runningChanged);
	/// {response (ms), bounce, curve ([x1, y1, x2, y2] or nothing)}, as IrisStyle.springFor gives it.
	/// `emerge` moves toward a larger value, `recede` toward a smaller one (null: emerge).
	Q_PROPERTY(QJSValue emerge READ emerge WRITE setEmerge NOTIFY emergeChanged);
	Q_PROPERTY(QJSValue recede READ recede WRITE setRecede NOTIFY recedeChanged);
	Q_PROPERTY(bool animate READ animate WRITE setAnimate NOTIFY animateChanged);
	Q_PROPERTY(qreal minimum MEMBER mMinimum NOTIFY minimumChanged);
	Q_PROPERTY(qreal epsilon MEMBER mEpsilon NOTIFY epsilonChanged);
	QML_ELEMENT;

public:
	explicit SpringValue(QObject* parent = nullptr): QObject(parent), ticker(this) {}

	[[nodiscard]] qreal to() const { return this->mTo; }
	[[nodiscard]] qreal value() const { return this->mValue; }
	[[nodiscard]] qreal velocity() const { return this->mVelocity; }
	[[nodiscard]] bool isRunning() const { return this->ticker.state() == QAbstractAnimation::Running; }
	[[nodiscard]] QJSValue emerge() const { return this->mEmerge; }
	[[nodiscard]] QJSValue recede() const { return this->mRecede; }
	[[nodiscard]] bool animate() const { return this->mAnimate; }

	void setTo(qreal to) {
		if (to == this->mTo) return;
		this->mTo = to;
		emit this->toChanged();
		if (this->complete) this->kick();
	}

	void setVelocity(qreal velocity) {
		if (velocity == this->mVelocity) return;
		this->mVelocity = velocity;
		emit this->velocityChanged();
	}

	void setEmerge(const QJSValue& params) {
		this->mEmerge = params;
		this->reparam();
		emit this->emergeChanged();
	}

	void setRecede(const QJSValue& params) {
		this->mRecede = params;
		this->reparam();
		emit this->recedeChanged();
	}

	void setAnimate(bool animate) {
		if (animate == this->mAnimate) return;
		this->mAnimate = animate;
		emit this->animateChanged();
		if (!animate) this->jump();
	}

	Q_INVOKABLE void jump() {
		bool was = this->isRunning();
		this->ticker.stop();
		this->segments.clear();
		this->setVelocity(0);
		this->target = this->mTo;
		this->setValue(this->mTo);
		if (was) emit this->runningChanged();
	}

	void classBegin() override {}
	void componentComplete() override {
		this->complete = true;
		this->target = this->mTo;
		this->setValue(this->mTo);
	}

	void tick() {
		qint64 now = qsrs::frameclock::step_ns > 0 ? qsrs::frameclock::step_ns : qsrs::frameclock::now_ns();
		qint64 frame = qsrs::frameclock::period_ns > 0 ? qsrs::frameclock::period_ns : 16'000'000;
		if (this->lastNs == 0) this->lastNs = now - qBound<qint64>(0, now - this->kickNs, frame);
		double dt = qMin(0.034, double(now - this->lastNs) / 1e9);
		if (dt <= 0) return;
		this->lastNs = now;
		this->step(dt);
	}

signals:
	void toChanged();
	void valueChanged();
	void velocityChanged();
	void runningChanged();
	void emergeChanged();
	void recedeChanged();
	void animateChanged();
	void minimumChanged();
	void epsilonChanged();

private:
	struct Params {
		double response = 0;
		double bounce = 0;
		QList<double> curve;
	};
	struct Segment {
		double delta;
		double age;
		double response;
		QList<double> curve;
	};

	static Params read(const QJSValue& v) {
		Params p;
		if (!v.isObject()) return p;
		p.response = v.property("response").toNumber();
		p.bounce = v.property("bounce").toNumber();
		if (std::isnan(p.response)) p.response = 0;
		if (std::isnan(p.bounce)) p.bounce = 0;
		QJSValue curve = v.property("curve");
		if (curve.isArray() && curve.property("length").toInt() >= 4)
			for (int i = 0; i < 4; i++) p.curve.append(curve.property(i).toNumber());
		return p;
	}

	[[nodiscard]] Params paramsFor(bool rising) const {
		return read(!rising && this->mRecede.isObject() ? this->mRecede : this->mEmerge);
	}

	// A parameter change mid-flight applies to the movement under way (IrisSpring forgets its
	// cache and reads the new ones on the next tick).
	void reparam() {
		if (this->isRunning()) this->active = this->paramsFor(this->rising);
	}

	void setValue(qreal value) {
		if (value == this->mValue) return;
		this->mValue = value;
		emit this->valueChanged();
	}

	void kick() {
		this->rising = this->mTo >= this->mValue;
		Params p = this->paramsFor(this->rising);
		if (!this->mAnimate || p.response <= 0) return this->jump();
		if (std::abs(this->mValue - this->mTo) < this->mEpsilon
		    && std::abs(this->mVelocity) < this->mEpsilon * 12)
			return this->jump();
		if (!p.curve.isEmpty()) {
			double delta = this->mTo - (this->segments.isEmpty() ? this->mValue : this->target);
			if (std::abs(delta) >= this->mEpsilon) this->segments.append({delta, 0, p.response, p.curve});
		} else {
			this->segments.clear();
		}
		this->target = this->mTo;
		this->active = p;
		if (this->isRunning()) return;
		this->lastNs = 0;
		this->kickNs = qsrs::frameclock::now_ns();
		this->ticker.start();
		emit this->runningChanged();
	}

	static double bezier(const QList<double>& c, double x) {
		if (x <= 0) return 0;
		if (x >= 1) return 1;
		double cx = 3 * c[0], bx = 3 * (c[2] - c[0]) - cx, ax = 1 - cx - bx;
		double cy = 3 * c[1], by = 3 * (c[3] - c[1]) - cy, ay = 1 - cy - by;
		double t = x;
		for (int i = 0; i < 8; i++) {
			double fx = ((ax * t + bx) * t + cx) * t - x;
			double d = (3 * ax * t + 2 * bx) * t + cx;
			if (std::abs(fx) < 1e-5 || std::abs(d) < 1e-6) break;
			t = qBound(0.0, t - fx / d, 1.0);
		}
		return ((ay * t + by) * t + cy) * t;
	}

	void step(double dt) {
		const Params& p = this->active;
		if (p.response <= 0) return this->jump();

		if (!this->segments.isEmpty() || !p.curve.isEmpty()) {
			double offset = 0;
			for (auto it = this->segments.begin(); it != this->segments.end();) {
				it->age += dt * 1000;
				if (it->age >= it->response) {
					it = this->segments.erase(it);
					continue;
				}
				offset += it->delta * (1 - bezier(it->curve, it->age / it->response));
				++it;
			}
			if (!this->segments.isEmpty() || !p.curve.isEmpty()) {
				if (this->segments.isEmpty()) return this->jump();
				double next = qMax(this->mMinimum, this->mTo - offset);
				this->setVelocity((next - this->mValue) / dt);
				this->setValue(next);
				return;
			}
			// A curve movement retargeted with a physical spring: the spring takes over at its target.
			this->setValue(this->mTo - offset);
		}

		double w = 2 * M_PI / (p.response / 1000);
		double zeta = 1 - p.bounce;
		double x0 = this->mValue - this->mTo;
		double v0 = this->mVelocity;
		double x, v;
		if (zeta < 0.9999) {
			double wd = w * std::sqrt(1 - zeta * zeta);
			double e = std::exp(-zeta * w * dt);
			double b = (v0 + zeta * w * x0) / wd;
			double c = std::cos(wd * dt), s = std::sin(wd * dt);
			x = e * (x0 * c + b * s);
			v = -zeta * w * x + e * (-x0 * wd * s + b * wd * c);
		} else {
			double e = std::exp(-w * dt);
			double b = v0 + w * x0;
			x = (x0 + b * dt) * e;
			v = (b - w * (x0 + b * dt)) * e;
		}
		double next = this->mTo + x;
		if (next < this->mMinimum) {
			next = this->mMinimum;
			v = 0;
		}
		if (std::abs(x) < this->mEpsilon && std::abs(v) < this->mEpsilon * 12) return this->jump();
		this->setVelocity(v);
		this->setValue(next);
	}

	QsSpringTicker ticker;
	qreal mTo = 0;
	qreal mValue = 0;
	qreal mVelocity = 0;
	qreal mMinimum = -1e9;
	qreal mEpsilon = 0.0005;
	bool mAnimate = true;
	bool complete = false;
	bool rising = true;
	QJSValue mEmerge;
	QJSValue mRecede;
	Params active;
	double target = 0;
	QList<Segment> segments;
	qint64 lastNs = 0;
	qint64 kickNs = 0;
};

inline QsSpringTicker::QsSpringTicker(SpringValue* spring): spring(spring) {}
inline void QsSpringTicker::updateCurrentTime(int) { this->spring->tick(); }
