#pragma once
// Reload plumbing from upstream core/reload.cpp and core/generation.cpp, first-load part only.
// Reloadables wait for the engine's generation to finish creating the root object, then reload
// top-down (reloadRecursive from the root); ones created later reload on the next event loop
// turn unless a parent reloads them first. The generation also owns upstream's event-loop driven
// incubation controller, which async LazyLoaders need to make progress.
#include <QtCore/QBasicTimer>
#include <QtCore/QObject>
#include <QtCore/QPointer>
#include <QtCore/QTimer>
#include <QtGui/QGuiApplication>
#include <QtGui/QScreen>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlIncubationController>
#include <QtQml/QQmlParserStatus>
#include <QtQml/qqml.h>

class QsIncubationController: public QObject, public QQmlIncubationController {
	Q_OBJECT;

public:
	explicit QsIncubationController(QObject* parent = nullptr): QObject(parent) {
		auto* app = qobject_cast<QGuiApplication*>(QGuiApplication::instance());
		QObject::connect(app, &QGuiApplication::screenAdded, this, &QsIncubationController::updateIncubationTime);
		QObject::connect(app, &QGuiApplication::screenRemoved, this, &QsIncubationController::updateIncubationTime);
		this->updateIncubationTime();
	}

protected:
	void incubatingObjectCountChanged(int count) override {
		if (count != 0) this->incubateLater();
	}

	void timerEvent(QTimerEvent* /*event*/) override {
		this->timer.stop();
		this->incubate();
	}

private:
	void incubateLater() {
		// Wait a while before the next batch so system events are not starved.
		if (!this->timer.isActive()) this->timer.start(this->incubationTime, this);
	}

	void incubate() {
		if (this->incubatingObjectCount() == 0) return;
		this->incubateFor(10);
		if (this->incubatingObjectCount() != 0) this->incubateLater();
	}

	void updateIncubationTime() {
		auto* screen = QGuiApplication::primaryScreen();
		if (screen == nullptr) return;
		// 1/3 frame on the primary screen
		this->incubationTime = qMax(1, static_cast<int>(1000 / screen->refreshRate() / 3));
	}

	QBasicTimer timer;
	int incubationTime = 5;
};

class QsGeneration: public QObject {
	Q_OBJECT;

public:
	explicit QsGeneration(QQmlEngine* engine): QObject(engine), incubation(this) {
		engine->setIncubationController(&this->incubation);
	}

	static QsGeneration* forEngine(QQmlEngine* engine) {
		if (engine == nullptr) return nullptr;
		auto* gen = engine->property("_qsGeneration").value<QsGeneration*>();
		if (gen == nullptr) {
			gen = new QsGeneration(engine);
			engine->setProperty("_qsGeneration", QVariant::fromValue(gen));
		}
		return gen;
	}

	static QsGeneration* forObject(QObject* object) {
		auto* context = QQmlEngine::contextForObject(object);
		return context == nullptr ? nullptr : forEngine(context->engine());
	}

	// Called by the host once the root object is fully created (upstream EngineGeneration::onReload).
	void finishLoad(QObject* root);

	bool reloadComplete = false;

signals:
	void reloadFinished();

private:
	QsIncubationController incubation;
};

class Reloadable: public QObject, public QQmlParserStatus {
	Q_OBJECT;
	Q_INTERFACES(QQmlParserStatus);
	// Matches objects across reloads. Kept for API compatibility; qs-rs does not hot reload yet.
	Q_PROPERTY(QString reloadableId MEMBER mReloadableId);
	QML_ANONYMOUS;

public:
	explicit Reloadable(QObject* parent = nullptr): QObject(parent) {}

	void classBegin() override {}

	void componentComplete() override {
		this->generation = QsGeneration::forObject(this);
		if (this->generation == nullptr) return;
		if (this->generation->reloadComplete) {
			// Delayed because Component.onCompleted runs after QQmlParserStatus::componentComplete.
			QTimer::singleShot(0, this, [this]() { this->reload(); });
		} else {
			QObject::connect(this->generation, &QsGeneration::reloadFinished, this, [this]() { this->reload(); });
		}
	}

	void reload() {
		if (this->reloadComplete) return;
		this->onReload();
		this->reloadComplete = true;
		if (this->generation != nullptr) QObject::disconnect(this->generation, &QsGeneration::reloadFinished, this, nullptr);
	}

	// Reloads every Reloadable under `object`, looking through objects that are not Reloadable.
	static void reloadRecursive(QObject* object) {
		if (object == nullptr) return;
		if (auto* reloadable = qobject_cast<Reloadable*>(object)) reloadable->reload();
		else for (auto* child: object->children()) reloadRecursive(child);
	}

protected:
	virtual void onReload() {
		for (auto* child: this->children()) reloadRecursive(child);
	}

	bool reloadComplete = false;

private:
	QPointer<QsGeneration> generation;
	QString mReloadableId;
};

inline void QsGeneration::finishLoad(QObject* root) {
	Reloadable::reloadRecursive(root);
	this->reloadComplete = true;
	emit this->reloadFinished();
}

namespace qsrs {
// Host side: installs the generation (and its incubation controller) before the root is created.
inline void install_generation(QQmlEngine& engine) { QsGeneration::forEngine(&engine); }
inline void finish_load(QQmlEngine& engine, QObject* root) { QsGeneration::forEngine(&engine)->finishLoad(root); }
} // namespace qsrs
