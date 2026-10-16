#pragma once
// LazyLoader, ported from upstream core/lazyloader.cpp. `active` and `loading` reflect the
// loader's state (item present, incubation running) while staying writable, so a QML binding on
// them is never replaced by the loader's own changes. Nothing is created before the shell
// finished loading (Reloadable::onReload).
#include <QtCore/QObject>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlIncubator>
#include <QtQml/qqml.h>
#include "reload.h"

class QsQmlIncubator: public QObject, public QQmlIncubator {
	Q_OBJECT;

public:
	explicit QsQmlIncubator(QQmlIncubator::IncubationMode mode, QObject* parent = nullptr)
	    : QObject(parent), QQmlIncubator(mode) {}

signals:
	void completed();
	void failed();

protected:
	void statusChanged(QQmlIncubator::Status status) override {
		if (status == QQmlIncubator::Ready) emit this->completed();
		else if (status == QQmlIncubator::Error) emit this->failed();
	}
};

class LazyLoader: public Reloadable {
	Q_OBJECT;
	Q_PROPERTY(QObject* item READ item NOTIFY itemChanged);
	Q_PROPERTY(bool loading READ isLoading WRITE setLoading NOTIFY loadingChanged);
	Q_PROPERTY(bool active READ isActive WRITE setActive NOTIFY activeChanged);
	Q_PROPERTY(bool activeAsync READ isActive WRITE setActiveAsync NOTIFY activeChanged);
	Q_PROPERTY(QQmlComponent* component READ component WRITE setComponent NOTIFY componentChanged);
	Q_PROPERTY(QString source READ source WRITE setSource NOTIFY sourceChanged);
	Q_CLASSINFO("DefaultProperty", "component");
	QML_ELEMENT;

public:
	explicit LazyLoader(QObject* parent = nullptr): Reloadable(parent) {}

	QObject* item() {
		if (this->isLoading()) this->setActive(true);
		return this->mItem;
	}

	[[nodiscard]] bool isLoading() const { return this->incubator != nullptr; }

	void setLoading(bool loading) {
		if (loading == this->targetLoading || this->isActive()) return;
		this->targetLoading = loading;
		if (loading) {
			this->incubateIfReady();
		} else if (this->mItem != nullptr) {
			this->mItem->deleteLater();
			this->mItem = nullptr;
		} else if (this->incubator != nullptr) {
			delete this->incubator;
			this->incubator = nullptr;
		}
	}

	[[nodiscard]] bool isActive() const { return this->mItem != nullptr; }

	void setActive(bool active) {
		if (active == this->targetActive) return;
		this->targetActive = active;
		if (active) {
			if (this->isLoading()) this->incubator->forceCompletion();
			else if (!this->isActive()) this->incubateIfReady();
		} else if (this->isActive()) {
			this->setItem(nullptr);
		}
	}

	void setActiveAsync(bool active) {
		if (active == (this->targetActive || this->targetLoading)) return;
		if (active) this->setLoading(true);
		else this->setActive(false);
	}

	[[nodiscard]] QQmlComponent* component() const { return this->cleanupComponent ? nullptr : this->mComponent; }

	void setComponent(QQmlComponent* component) {
		if (this->cleanupComponent) this->setSource(QString());
		if (component == this->mComponent) return;
		this->cleanupComponent = false;
		if (this->mComponent != nullptr) QObject::disconnect(this->mComponent, nullptr, this, nullptr);
		this->mComponent = component;
		if (component != nullptr) {
			QObject::connect(component, &QObject::destroyed, this, [this]() { this->mComponent = nullptr; });
		}
		emit this->componentChanged();
	}

	[[nodiscard]] QString source() const { return this->mSource; }

	void setSource(QString source) {
		if (!this->cleanupComponent) this->setComponent(nullptr);
		if (source == this->mSource) return;
		this->cleanupComponent = true;
		this->mSource = std::move(source);
		delete this->mComponent;
		this->mComponent = nullptr;
		if (!this->mSource.isEmpty()) {
			auto* context = QQmlEngine::contextForObject(this);
			this->mComponent = new QQmlComponent(
			    context == nullptr ? nullptr : context->engine(),
			    context == nullptr ? QUrl(this->mSource) : context->resolvedUrl(QUrl(this->mSource))
			);
			if (this->mComponent->isError()) {
				qWarning().noquote() << this->mComponent->errorString();
				delete this->mComponent;
				this->mComponent = nullptr;
			}
		}
		emit this->sourceChanged();
	}

signals:
	void activeChanged();
	void loadingChanged();
	void itemChanged();
	void sourceChanged();
	void componentChanged();

protected:
	void onReload() override {
		this->incubateIfReady(true);
		if (this->mItem != nullptr) Reloadable::reloadRecursive(this->mItem);
	}

private:
	void setItem(QObject* item) {
		if (item == this->mItem) return;
		if (this->mItem != nullptr) this->mItem->deleteLater();
		this->mItem = item;
		if (item != nullptr) item->setParent(this);
		this->targetActive = this->isActive();
		emit this->itemChanged();
		emit this->activeChanged();
	}

	void incubateIfReady(bool overrideReloadCheck = false) {
		if (!(this->reloadComplete || overrideReloadCheck) || !(this->targetLoading || this->targetActive)
		    || this->mComponent == nullptr || this->incubator != nullptr)
		{
			return;
		}
		this->incubator = new QsQmlIncubator(
		    this->targetActive ? QQmlIncubator::Synchronous : QQmlIncubator::Asynchronous,
		    this
		);
		QObject::connect(this->incubator, &QsQmlIncubator::completed, this, &LazyLoader::onIncubationCompleted);
		QObject::connect(this->incubator, &QsQmlIncubator::failed, this, &LazyLoader::onIncubationFailed);
		emit this->loadingChanged();
		this->mComponent->create(*this->incubator, QQmlEngine::contextForObject(this->mComponent));
	}

	void onIncubationCompleted() {
		this->setItem(this->incubator->object());
		// The incubator is not necessarily inert at the time of this callback.
		this->incubator->deleteLater();
		this->incubator = nullptr;
		this->targetLoading = false;
		emit this->loadingChanged();
	}

	void onIncubationFailed() {
		qWarning() << "Failed to create LazyLoader component";
		for (auto& error: this->incubator->errors()) qWarning() << error;
		this->incubator->deleteLater();
		this->incubator = nullptr;
		this->targetLoading = false;
		emit this->loadingChanged();
	}

	bool targetLoading = false;
	bool targetActive = false;
	QObject* mItem = nullptr;
	QString mSource;
	QQmlComponent* mComponent = nullptr;
	QsQmlIncubator* incubator = nullptr;
	bool cleanupComponent = false;
};
