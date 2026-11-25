#pragma once
// SpringValue: IrisSpring (inir modules/iris/components/IrisSpring.qml) backed by the Rust spring
// integrator (src/spring.rs), stepped by the animation driver at the time each frame is meant to be
// on screen.
#include <QtCore/QAbstractAnimation>
#include <QtCore/QObject>
#include <QtQml/QJSValue>
#include <QtQml/QQmlParserStatus>
#include <QtQml/qqml.h>
#include <cmath>
#include "frameclock.h"
#include "quickshell/src/spring.cxx.h"

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
	explicit SpringValue(QObject* parent = nullptr)
	    : QObject(parent), ticker(this), engine(qsrs::spring::new_spring()) {}

	[[nodiscard]] qreal to() const { return this->mTo; }
	[[nodiscard]] qreal value() const { return this->engine->value(); }
	[[nodiscard]] qreal velocity() const { return this->engine->velocity(); }
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
		if (this->engine->set_velocity(velocity)) emit this->velocityChanged();
	}

	void setEmerge(const QJSValue& params) {
		this->mEmerge = params;
		this->emergeParams = readParams(params);
		this->reparam();
		emit this->emergeChanged();
	}

	void setRecede(const QJSValue& params) {
		this->mRecede = params;
		this->hasRecede = params.isObject();
		this->recedeParams = this->hasRecede ? readParams(params) : qsrs::spring::SpringParams {};
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
		auto out = this->engine->jump(this->mTo);
		if (out.velocity_changed) emit this->velocityChanged();
		if (out.value_changed) emit this->valueChanged();
		if (was) emit this->runningChanged();
	}

	void classBegin() override {}
	void componentComplete() override {
		this->complete = true;
		if (this->engine->complete(this->mTo)) emit this->valueChanged();
	}

	void tick() {
		qint64 now = qsrs::frameclock::step_ns > 0 ? qsrs::frameclock::step_ns : qsrs::frameclock::now_ns();
		qint64 frame = qsrs::frameclock::period_ns > 0 ? qsrs::frameclock::period_ns : 16'000'000;
		if (this->lastNs == 0) this->lastNs = now - qBound<qint64>(0, now - this->kickNs, frame);
		double dt = qMin(0.034, double(now - this->lastNs) / 1e9);
		if (dt <= 0) return;
		this->lastNs = now;
		auto out = this->engine->step(dt, this->mTo, this->mMinimum, this->mEpsilon);
		if (out.settled) {
			bool was = this->isRunning();
			this->ticker.stop();
			if (out.velocity_changed) emit this->velocityChanged();
			if (out.value_changed) emit this->valueChanged();
			if (was) emit this->runningChanged();
			return;
		}
		if (out.velocity_changed) emit this->velocityChanged();
		if (out.value_changed) emit this->valueChanged();
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
	static qsrs::spring::SpringParams readParams(const QJSValue& v) {
		qsrs::spring::SpringParams p {};
		if (!v.isObject()) return p;
		p.response = v.property("response").toNumber();
		p.bounce = v.property("bounce").toNumber();
		if (std::isnan(p.response)) p.response = 0;
		if (std::isnan(p.bounce)) p.bounce = 0;
		QJSValue curve = v.property("curve");
		if (curve.isArray() && curve.property("length").toInt() >= 4) {
			p.has_curve = true;
			p.c0 = curve.property(0).toNumber();
			p.c1 = curve.property(1).toNumber();
			p.c2 = curve.property(2).toNumber();
			p.c3 = curve.property(3).toNumber();
		}
		return p;
	}

	void reparam() {
		if (!this->isRunning()) return;
		const auto& p = (!this->engine->rising() && this->hasRecede) ? this->recedeParams : this->emergeParams;
		this->engine->set_active(p);
	}

	void kick() {
		bool wasRunning = this->isRunning();
		auto out = this->engine->kick(
		    this->mTo, this->mAnimate, wasRunning, this->mMinimum, this->mEpsilon,
		    this->emergeParams, this->recedeParams, this->hasRecede
		);
		if (out.jumped) {
			this->ticker.stop();
			if (out.velocity_changed) emit this->velocityChanged();
			if (out.value_changed) emit this->valueChanged();
			if (wasRunning) emit this->runningChanged();
			return;
		}
		if (out.start_ticker) {
			this->lastNs = 0;
			this->kickNs = qsrs::frameclock::now_ns();
			this->ticker.start();
			emit this->runningChanged();
		}
	}

	QsSpringTicker ticker;
	rust::Box<qsrs::spring::SpringEngine> engine;
	qreal mTo = 0;
	qreal mMinimum = -1e9;
	qreal mEpsilon = 0.0005;
	bool mAnimate = true;
	bool complete = false;
	bool hasRecede = false;
	QJSValue mEmerge;
	QJSValue mRecede;
	qsrs::spring::SpringParams emergeParams {};
	qsrs::spring::SpringParams recedeParams {};
	qint64 lastNs = 0;
	qint64 kickNs = 0;
};

inline QsSpringTicker::QsSpringTicker(SpringValue* spring): spring(spring) {}
inline void QsSpringTicker::updateCurrentTime(int) { this->spring->tick(); }
