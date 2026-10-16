#pragma once
// DesktopEntries, DesktopEntry, DesktopAction (upstream core/desktopentry.{hpp,cpp} and
// desktopentrymonitor.cpp). Scanning and parsing are Rust (src/desktopentry.rs); these QObjects
// hold the result. The first scan is synchronous like upstream, rescans run on the global
// thread pool after a 100 ms debounce of directory changes.
#include <algorithm>

#include <QtCore/QDir>
#include <QtCore/QFileInfo>
#include <QtCore/QFileSystemWatcher>
#include <QtCore/QHash>
#include <QtCore/QObject>
#include <QtCore/QThreadPool>
#include <QtCore/QTimer>
#include <QtQml/qqml.h>

#include "objectmodel.h"
#include "quickshell/src/desktopentry.cxx.h"

namespace qsrs::desktop {

inline QString qstr(const rust::String& s) { return QString::fromUtf8(s.data(), static_cast<qsizetype>(s.size())); }

inline QList<QString> qlist(const rust::Vec<rust::String>& v) {
	QList<QString> out;
	out.reserve(static_cast<qsizetype>(v.size()));
	for (const auto& s: v) out.append(qstr(s));
	return out;
}

struct ActionData {
	QString id, name, icon, execString;
	QList<QString> command;
};

struct EntryData {
	QString id, name, genericName, startupClass, comment, icon, execString, workingDirectory;
	bool noDisplay = false, hidden = false, terminal = false;
	QList<QString> command, categories, keywords;
	QList<ActionData> actions;
};

inline QList<EntryData> scan() {
	QList<EntryData> out;
	for (const auto& d: desktop_scan()) {
		EntryData e {
		    qstr(d.id), qstr(d.name), qstr(d.generic_name), qstr(d.startup_class), qstr(d.comment),
		    qstr(d.icon), qstr(d.exec_string), qstr(d.working_directory), d.no_display, d.hidden,
		    d.terminal, qlist(d.command), qlist(d.categories), qlist(d.keywords), {}
		};
		for (const auto& a: d.actions) {
			e.actions.append({qstr(a.id), qstr(a.name), qstr(a.icon), qstr(a.exec_string), qlist(a.command)});
		}
		out.append(std::move(e));
	}
	return out;
}

inline void exec(const QList<QString>& command, const QString& workingDirectory) {
	rust::Vec<rust::String> args;
	for (const auto& s: command) args.push_back(rust::String(s.toStdString()));
	desktop_exec(args, rust::Str(workingDirectory.toStdString()));
}

} // namespace qsrs::desktop

#define QSRS_DE_PROP(Type, name, Name)                                                              \
public:                                                                                            \
	[[nodiscard]] Type name() const { return this->m##Name; }                                       \
	void set##Name(const Type& v) {                                                                 \
		if (this->m##Name == v) return;                                                             \
		this->m##Name = v;                                                                          \
		emit this->name##Changed();                                                                 \
	}                                                                                              \
                                                                                                   \
private:                                                                                           \
	Type m##Name {};

class DesktopEntry;

class DesktopAction: public QObject {
	Q_OBJECT;
	Q_PROPERTY(QString id READ id CONSTANT);
	Q_PROPERTY(QString name READ name NOTIFY nameChanged);
	Q_PROPERTY(QString icon READ icon NOTIFY iconChanged);
	Q_PROPERTY(QString execString READ execString NOTIFY execStringChanged);
	Q_PROPERTY(QList<QString> command READ command NOTIFY commandChanged);
	QML_ELEMENT;
	QML_UNCREATABLE("DesktopAction instances must be retrieved from a DesktopEntry");

public:
	explicit DesktopAction(QString id, QObject* entry): QObject(entry), mId(std::move(id)) {}
	[[nodiscard]] QString id() const { return this->mId; }
	Q_INVOKABLE void execute() const;

signals:
	void nameChanged();
	void iconChanged();
	void execStringChanged();
	void commandChanged();

private:
	QString mId;
	QSRS_DE_PROP(QString, name, Name)
	QSRS_DE_PROP(QString, icon, Icon)
	QSRS_DE_PROP(QString, execString, ExecString)
	QSRS_DE_PROP(QList<QString>, command, Command)
};

class DesktopEntry: public QObject {
	Q_OBJECT;
	Q_PROPERTY(QString id READ id CONSTANT);
	Q_PROPERTY(QString name READ name NOTIFY nameChanged);
	Q_PROPERTY(QString genericName READ genericName NOTIFY genericNameChanged);
	Q_PROPERTY(QString startupClass READ startupClass NOTIFY startupClassChanged);
	Q_PROPERTY(bool noDisplay READ noDisplay NOTIFY noDisplayChanged);
	Q_PROPERTY(QString comment READ comment NOTIFY commentChanged);
	Q_PROPERTY(QString icon READ icon NOTIFY iconChanged);
	Q_PROPERTY(QString execString READ execString NOTIFY execStringChanged);
	Q_PROPERTY(QList<QString> command READ command NOTIFY commandChanged);
	Q_PROPERTY(QString workingDirectory READ workingDirectory NOTIFY workingDirectoryChanged);
	Q_PROPERTY(bool runInTerminal READ runInTerminal NOTIFY runInTerminalChanged);
	Q_PROPERTY(QList<QString> categories READ categories NOTIFY categoriesChanged);
	Q_PROPERTY(QList<QString> keywords READ keywords NOTIFY keywordsChanged);
	Q_PROPERTY(QList<DesktopAction*> actions READ actions CONSTANT);
	QML_ELEMENT;
	QML_UNCREATABLE("DesktopEntry instances must be retrieved from DesktopEntries");

public:
	explicit DesktopEntry(QString id, QObject* parent): QObject(parent), mId(std::move(id)) {}
	[[nodiscard]] QString id() const { return this->mId; }
	[[nodiscard]] QList<DesktopAction*> actions() const { return this->mActions; }
	[[nodiscard]] bool isValid() const { return !this->mName.isEmpty(); }

	Q_INVOKABLE void execute() const { qsrs::desktop::exec(this->mCommand, this->mWorkingDirectory); }

	void updateState(const qsrs::desktop::EntryData& d) {
		this->setName(d.name);
		this->setGenericName(d.genericName);
		this->setStartupClass(d.startupClass);
		this->setNoDisplay(d.noDisplay);
		this->setComment(d.comment);
		this->setIcon(d.icon);
		this->setExecString(d.execString);
		this->setCommand(d.command);
		this->setWorkingDirectory(d.workingDirectory);
		this->setRunInTerminal(d.terminal);
		this->setCategories(d.categories);
		this->setKeywords(d.keywords);

		auto old = this->mActions;
		this->mActions.clear();
		for (const auto& a: d.actions) {
			DesktopAction* act = nullptr;
			auto found = std::find_if(old.begin(), old.end(), [&](DesktopAction* x) { return x->id() == a.id; });
			if (found != old.end()) {
				act = *found;
				old.erase(found);
			} else {
				act = new DesktopAction(a.id, this);
			}
			act->setName(a.name);
			act->setIcon(a.icon);
			act->setExecString(a.execString);
			act->setCommand(a.command);
			this->mActions.append(act);
		}
		for (auto* leftover: old) leftover->deleteLater();
	}

signals:
	void nameChanged();
	void genericNameChanged();
	void startupClassChanged();
	void noDisplayChanged();
	void commentChanged();
	void iconChanged();
	void execStringChanged();
	void commandChanged();
	void workingDirectoryChanged();
	void runInTerminalChanged();
	void categoriesChanged();
	void keywordsChanged();

private:
	QString mId;
	QList<DesktopAction*> mActions;
	QSRS_DE_PROP(QString, name, Name)
	QSRS_DE_PROP(QString, genericName, GenericName)
	QSRS_DE_PROP(QString, startupClass, StartupClass)
	QSRS_DE_PROP(bool, noDisplay, NoDisplay)
	QSRS_DE_PROP(QString, comment, Comment)
	QSRS_DE_PROP(QString, icon, Icon)
	QSRS_DE_PROP(QString, execString, ExecString)
	QSRS_DE_PROP(QList<QString>, command, Command)
	QSRS_DE_PROP(QString, workingDirectory, WorkingDirectory)
	QSRS_DE_PROP(bool, runInTerminal, RunInTerminal)
	QSRS_DE_PROP(QList<QString>, categories, Categories)
	QSRS_DE_PROP(QList<QString>, keywords, Keywords)
};

inline void DesktopAction::execute() const {
	auto* entry = qobject_cast<DesktopEntry*>(this->parent());
	qsrs::desktop::exec(this->mCommand, entry ? entry->workingDirectory() : QString());
}

// Process-wide, shared by every engine (upstream: DesktopEntryManager::instance()).
class QsDesktopEntryManager: public QObject {
	Q_OBJECT;

public:
	static QsDesktopEntryManager* instance() {
		static auto* manager = new QsDesktopEntryManager(); // NOLINT
		return manager;
	}

	QsObjectModel* applications() { return &this->mApplications; }

	DesktopEntry* byId(const QString& id) const {
		if (auto* e = this->entries.value(id)) return e;
		return this->lowercaseEntries.value(id.toLower());
	}

	DesktopEntry* heuristicLookup(const QString& name) const {
		if (auto* e = this->byId(name)) return e;
		for (auto* e: this->order)
			if (e->startupClass() == name) return e;
		auto lower = name.toLower();
		for (auto* e: this->order)
			if (e->startupClass().toLower() == lower) return e;
		return nullptr;
	}

signals:
	void applicationsChanged();

private:
	QsDesktopEntryManager() {
		this->debounce.setSingleShot(true);
		this->debounce.setInterval(100);
		QObject::connect(&this->watcher, &QFileSystemWatcher::directoryChanged, &this->debounce, qOverload<>(&QTimer::start));
		QObject::connect(&this->debounce, &QTimer::timeout, this, &QsDesktopEntryManager::rescan);
		for (const auto& p: desktop_paths()) this->watch(qsrs::desktop::qstr(p));
		this->apply(qsrs::desktop::scan());
	}

	// The directory, its subdirectories and its parents (so a created applications dir is seen).
	void watch(const QString& path) {
		if (!QDir(path).exists()) return;
		this->watcher.addPath(path);
		for (auto p = QFileInfo(path).absolutePath(); !p.isEmpty();) {
			this->watcher.addPath(p);
			auto parent = QFileInfo(p).dir().absolutePath();
			if (parent == p) break;
			p = parent;
		}
		for (const auto& sub: QDir(path).entryInfoList(QDir::Dirs | QDir::NoDotAndDotDot | QDir::NoSymLinks))
			this->watcher.addPath(sub.absoluteFilePath());
	}

	void rescan() {
		if (this->scanning) {
			this->queued = true;
			return;
		}
		this->scanning = true;
		this->queued = false;
		QThreadPool::globalInstance()->start([this] {
			auto result = qsrs::desktop::scan();
			QMetaObject::invokeMethod(this, [this, result = std::move(result)] {
				this->apply(result);
				this->scanning = false;
				if (this->queued) this->rescan();
			}, Qt::QueuedConnection);
		});
	}

	void apply(const QList<qsrs::desktop::EntryData>& results) {
		// Resolve overrides first (later = higher priority, Hidden masks, nameless entries don't
		// replace a valid one), so a kept id keeps its object. Upstream recreates overridden ids on
		// every rescan.
		QList<const qsrs::desktop::EntryData*> winners;
		QHash<QString, qsizetype> slot;
		for (const auto& d: results) {
			if (!d.hidden && d.name.isEmpty()) continue;
			if (auto it = slot.find(d.id); it != slot.end()) {
				winners[it.value()] = nullptr;
				slot.erase(it);
			}
			if (d.hidden) continue;
			slot.insert(d.id, winners.length());
			winners.append(&d);
		}
		auto old = this->entries;
		QHash<QString, DesktopEntry*> next;
		QHash<QString, DesktopEntry*> lower;
		QList<DesktopEntry*> nextOrder;
		for (const auto* d: winners) {
			if (d == nullptr) continue;
			auto* entry = old.take(d->id);
			if (entry == nullptr) entry = new DesktopEntry(d->id, this);
			entry->updateState(*d);
			next.insert(d->id, entry);
			nextOrder.append(entry);
			lower.insert(d->id.toLower(), entry);
		}
		this->entries = next;
		this->lowercaseEntries = lower;
		this->order = nextOrder;
		QList<QObject*> apps;
		for (auto* e: nextOrder)
			if (!e->noDisplay()) apps.append(e);
		this->mApplications.setValues(apps);
		emit this->applicationsChanged();
		for (auto* e: old) e->deleteLater();
	}

	QHash<QString, DesktopEntry*> entries;
	QHash<QString, DesktopEntry*> lowercaseEntries;
	// Scan order; upstream iterates its QHash, whose order is arbitrary.
	QList<DesktopEntry*> order;
	QsObjectModel mApplications {this};
	QFileSystemWatcher watcher;
	QTimer debounce;
	bool scanning = false;
	bool queued = false;
};

class DesktopEntries: public QObject {
	Q_OBJECT;
	Q_PROPERTY(QsObjectModel* applications READ applications CONSTANT);
	QML_ELEMENT;
	QML_SINGLETON;

public:
	explicit DesktopEntries(QObject* parent = nullptr): QObject(parent) {
		QObject::connect(QsDesktopEntryManager::instance(), &QsDesktopEntryManager::applicationsChanged, this, &DesktopEntries::applicationsChanged);
	}

	Q_INVOKABLE [[nodiscard]] static DesktopEntry* byId(const QString& id) { return QsDesktopEntryManager::instance()->byId(id); }
	Q_INVOKABLE [[nodiscard]] static DesktopEntry* heuristicLookup(const QString& name) {
		return QsDesktopEntryManager::instance()->heuristicLookup(name);
	}
	[[nodiscard]] static QsObjectModel* applications() { return QsDesktopEntryManager::instance()->applications(); }

signals:
	void applicationsChanged();
};
