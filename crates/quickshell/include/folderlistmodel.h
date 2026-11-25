#pragma once
// Drop-in replacement for Qt.labs.folderlistmodel.FolderListModel backed by Rust (`folderlist.rs`),
// sharing the single `notify-rs` inotify thread and bounded `qs-io` worker pool instead of spawning
// one `FileInfoThread` OS thread per QML FolderListModel instance.

#include <QtCore/QAbstractListModel>
#include <QtCore/QCoreApplication>
#include <QtCore/QDateTime>
#include <QtCore/QDir>
#include <QtCore/QFileInfo>
#include <QtCore/QHash>
#include <QtCore/QMetaObject>
#include <QtCore/QStringList>
#include <QtCore/QTimer>
#include <QtCore/QUrl>
#include <QtQml/QQmlParserStatus>
#include <QtQml/qqml.h>

#include <cstdint>
#include <vector>

#include "quickshell/src/folderlist.cxx.h"

class QsFolderListModel;

namespace qsrs::folderlist {

inline QString qstr(const rust::String& s) {
	return QString::fromUtf8(s.data(), static_cast<qsizetype>(s.size()));
}

struct Entry {
	QString fileName;
	QString filePath;
	QUrl fileUrl;
	QString fileBaseName;
	QString fileSuffix;
	qint64 fileSize = 0;
	qint64 fileModifiedMs = 0;
	qint64 fileAccessedMs = 0;
	bool fileIsDir = false;

	bool operator==(const Entry& o) const {
		return fileName == o.fileName && filePath == o.filePath && fileSize == o.fileSize
		    && fileModifiedMs == o.fileModifiedMs && fileIsDir == o.fileIsDir;
	}
};

// GUI-thread-only map from Rust model id to live instance: avoids raw pointers across threads.
QHash<std::uint64_t, QsFolderListModel*>& registry();

} // namespace qsrs::folderlist

extern "C" void qsrs_folderlist_dir_changed(std::uint64_t id);
extern "C" void qsrs_folderlist_scan_done(std::uint64_t id, std::uint64_t gen);
extern "C" void qsrs_register_folder_list_model(const char* import_path_utf8);

class QsFolderListModel: public QAbstractListModel, public QQmlParserStatus {
	Q_OBJECT
	Q_INTERFACES(QQmlParserStatus)

	Q_PROPERTY(QUrl folder READ folder WRITE setFolder NOTIFY folderChanged FINAL)
	Q_PROPERTY(QUrl rootFolder READ rootFolder WRITE setRootFolder NOTIFY rootFolderChanged FINAL)
	Q_PROPERTY(QUrl parentFolder READ parentFolder NOTIFY folderChanged FINAL)
	Q_PROPERTY(QStringList nameFilters READ nameFilters WRITE setNameFilters NOTIFY nameFilterChanged FINAL)
	Q_PROPERTY(SortField sortField READ sortField WRITE setSortField NOTIFY sortFieldChanged FINAL)
	Q_PROPERTY(bool sortReversed READ sortReversed WRITE setSortReversed NOTIFY sortReversedChanged FINAL)
	Q_PROPERTY(bool showFiles READ showFiles WRITE setShowFiles NOTIFY showFilesChanged FINAL)
	Q_PROPERTY(bool showDirs READ showDirs WRITE setShowDirs NOTIFY showDirsChanged FINAL)
	Q_PROPERTY(bool showDirsFirst READ showDirsFirst WRITE setShowDirsFirst NOTIFY showDirsFirstChanged FINAL)
	Q_PROPERTY(bool showDotAndDotDot READ showDotAndDotDot WRITE setShowDotAndDotDot NOTIFY showDotAndDotDotChanged FINAL)
	Q_PROPERTY(bool showDot READ showDot WRITE setShowDot NOTIFY showDotChanged)
	Q_PROPERTY(bool showDotDot READ showDotDot WRITE setShowDotDot NOTIFY showDotDotChanged)
	Q_PROPERTY(bool showHidden READ showHidden WRITE setShowHidden NOTIFY showHiddenChanged FINAL)
	Q_PROPERTY(bool showOnlyReadable READ showOnlyReadable WRITE setShowOnlyReadable NOTIFY showOnlyReadableChanged FINAL)
	Q_PROPERTY(bool caseSensitive READ caseSensitive WRITE setCaseSensitive NOTIFY caseSensitiveChanged FINAL)
	Q_PROPERTY(bool sortCaseSensitive READ sortCaseSensitive WRITE setSortCaseSensitive NOTIFY sortCaseSensitiveChanged FINAL)
	Q_PROPERTY(int count READ count NOTIFY countChanged FINAL)
	Q_PROPERTY(Status status READ status NOTIFY statusChanged FINAL)

public:
	enum SortField { Unsorted, Name, Time, Size, Type };
	Q_ENUM(SortField)

	enum Status { Null, Ready, Loading };
	Q_ENUM(Status)

	enum Roles {
		FileNameRole = Qt::UserRole + 1,
		FilePathRole,
		FileBaseNameRole,
		FileSuffixRole,
		FileSizeRole,
		FileLastModifiedRole,
		FileLastReadRole,
		FileIsDirRole,
		FileUrlRole,
		FileURLRole
	};

	explicit QsFolderListModel(QObject* parent = nullptr): QAbstractListModel(parent) {
		this->mFolderPath = QDir::cleanPath(QDir::currentPath());
		this->mFolder = QUrl::fromLocalFile(this->mFolderPath);
		this->mId = folder_watch_register(
		    reinterpret_cast<std::size_t>(&qsrs_folderlist_dir_changed),
		    reinterpret_cast<std::size_t>(&qsrs_folderlist_scan_done));
		qsrs::folderlist::registry().insert(this->mId, this);

		this->mScanDebounce.setSingleShot(true);
		this->mScanDebounce.setInterval(0);
		QObject::connect(&this->mScanDebounce, &QTimer::timeout, this, [this] { this->startScan(false); });

		this->mWatchDebounce.setSingleShot(true);
		this->mWatchDebounce.setInterval(40);
		QObject::connect(&this->mWatchDebounce, &QTimer::timeout, this, [this] { this->startScan(true); });
	}

	~QsFolderListModel() override {
		qsrs::folderlist::registry().remove(this->mId);
		folder_watch_unregister(this->mId);
	}

	void classBegin() override { this->mComplete = false; }

	void componentComplete() override {
		this->mComplete = true;
		this->syncWatch();
		this->startScan(false);
	}

	int rowCount(const QModelIndex& parent = QModelIndex()) const override {
		return parent.isValid() ? 0 : static_cast<int>(this->mEntries.size());
	}

	QVariant data(const QModelIndex& index, int role = Qt::DisplayRole) const override {
		if (!index.isValid() || index.row() < 0 || static_cast<std::size_t>(index.row()) >= this->mEntries.size()) {
			return {};
		}
		const auto& e = this->mEntries[static_cast<std::size_t>(index.row())];
		switch (role) {
		case Qt::DisplayRole:
		case FileNameRole: return e.fileName;
		case FilePathRole: return e.filePath;
		case FileBaseNameRole: return e.fileBaseName;
		case FileSuffixRole: return e.fileSuffix;
		case FileSizeRole: return e.fileSize;
		case FileLastModifiedRole: return QDateTime::fromMSecsSinceEpoch(e.fileModifiedMs);
		case FileLastReadRole: return QDateTime::fromMSecsSinceEpoch(e.fileAccessedMs);
		case FileIsDirRole: return e.fileIsDir;
		case FileUrlRole:
		case FileURLRole: return e.fileUrl;
		default: return {};
		}
	}

	QHash<int, QByteArray> roleNames() const override {
		static const QHash<int, QByteArray> roles = {
		    {FileNameRole, "fileName"},
		    {FilePathRole, "filePath"},
		    {FileBaseNameRole, "fileBaseName"},
		    {FileSuffixRole, "fileSuffix"},
		    {FileSizeRole, "fileSize"},
		    {FileLastModifiedRole, "fileModified"},
		    {FileLastReadRole, "fileAccessed"},
		    {FileIsDirRole, "fileIsDir"},
		    {FileUrlRole, "fileUrl"},
		    {FileURLRole, "fileURL"},
		};
		return roles;
	}

	[[nodiscard]] QUrl folder() const { return this->mFolder; }
	void setFolder(const QUrl& folder) {
		if (folder == this->mFolder) return;
		const QString local = urlToLocalPath(folder);
		if (local.isEmpty()) return;
		const QFileInfo info(local);
		if (!info.exists() || !info.isDir()) return;
		const QString clean = QDir::cleanPath(info.absoluteFilePath());
		const QUrl normalized = QUrl::fromLocalFile(clean);
		if (clean == this->mFolderPath && normalized == this->mFolder) return;
		this->mFolderPath = clean;
		this->mFolder = normalized;
		if (this->mComplete) {
			if (!this->mEntries.empty()) {
				this->beginResetModel();
				this->mEntries.clear();
				this->endResetModel();
				emit this->countChanged();
				emit this->rowCountChanged();
			}
			this->setStatus(Status::Loading);
			emit this->folderChanged();
			this->syncWatch();
			this->scheduleScan();
		} else {
			emit this->folderChanged();
		}
	}

	[[nodiscard]] QUrl rootFolder() const { return this->mRootFolder; }
	void setRootFolder(const QUrl& path) {
		if (path == this->mRootFolder) return;
		this->mRootFolder = path;
		emit this->rootFolderChanged();
		emit this->folderChanged();
	}

	[[nodiscard]] QUrl parentFolder() const {
		if (this->mFolderPath.isEmpty()) return {};
		if (!this->mRootFolder.isEmpty()) {
			const QString rootLocal = QDir::cleanPath(urlToLocalPath(this->mRootFolder));
			if (!rootLocal.isEmpty() && rootLocal == this->mFolderPath) return {};
		}
		QDir d(this->mFolderPath);
		if (!d.cdUp()) return {};
		return QUrl::fromLocalFile(QDir::cleanPath(d.absolutePath()));
	}

	[[nodiscard]] QStringList nameFilters() const { return this->mNameFilters; }
	void setNameFilters(const QStringList& filters) {
		if (filters == this->mNameFilters) return;
		this->mNameFilters = filters;
		emit this->nameFilterChanged();
		this->scheduleScan();
	}

	[[nodiscard]] SortField sortField() const { return this->mSortField; }
	void setSortField(SortField field) {
		if (field == this->mSortField) return;
		this->mSortField = field;
		emit this->sortFieldChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool sortReversed() const { return this->mSortReversed; }
	void setSortReversed(bool rev) {
		if (rev == this->mSortReversed) return;
		this->mSortReversed = rev;
		emit this->sortReversedChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showFiles() const { return this->mShowFiles; }
	void setShowFiles(bool v) {
		if (v == this->mShowFiles) return;
		this->mShowFiles = v;
		emit this->showFilesChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showDirs() const { return this->mShowDirs; }
	void setShowDirs(bool v) {
		if (v == this->mShowDirs) return;
		this->mShowDirs = v;
		emit this->showDirsChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showDirsFirst() const { return this->mShowDirsFirst; }
	void setShowDirsFirst(bool v) {
		if (v == this->mShowDirsFirst) return;
		this->mShowDirsFirst = v;
		emit this->showDirsFirstChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showDotAndDotDot() const { return this->mShowDot && this->mShowDotDot; }
	void setShowDotAndDotDot(bool v) {
		const bool dotChanged = (v != this->mShowDot);
		const bool dotDotChanged = (v != this->mShowDotDot);
		if (!dotChanged && !dotDotChanged) return;
		this->mShowDot = v;
		this->mShowDotDot = v;
		if (dotChanged) emit this->showDotChanged();
		if (dotDotChanged) emit this->showDotDotChanged();
		emit this->showDotAndDotDotChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showDot() const { return this->mShowDot; }
	void setShowDot(bool v) {
		if (v == this->mShowDot) return;
		const bool bothBefore = this->showDotAndDotDot();
		this->mShowDot = v;
		emit this->showDotChanged();
		if (bothBefore != this->showDotAndDotDot()) emit this->showDotAndDotDotChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showDotDot() const { return this->mShowDotDot; }
	void setShowDotDot(bool v) {
		if (v == this->mShowDotDot) return;
		const bool bothBefore = this->showDotAndDotDot();
		this->mShowDotDot = v;
		emit this->showDotDotChanged();
		if (bothBefore != this->showDotAndDotDot()) emit this->showDotAndDotDotChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showHidden() const { return this->mShowHidden; }
	void setShowHidden(bool v) {
		if (v == this->mShowHidden) return;
		this->mShowHidden = v;
		emit this->showHiddenChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool showOnlyReadable() const { return this->mShowOnlyReadable; }
	void setShowOnlyReadable(bool v) {
		if (v == this->mShowOnlyReadable) return;
		this->mShowOnlyReadable = v;
		emit this->showOnlyReadableChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool caseSensitive() const { return this->mCaseSensitive; }
	void setCaseSensitive(bool v) {
		if (v == this->mCaseSensitive) return;
		this->mCaseSensitive = v;
		emit this->caseSensitiveChanged();
		this->scheduleScan();
	}

	[[nodiscard]] bool sortCaseSensitive() const { return this->mSortCaseSensitive; }
	void setSortCaseSensitive(bool v) {
		if (v == this->mSortCaseSensitive) return;
		this->mSortCaseSensitive = v;
		emit this->sortCaseSensitiveChanged();
		this->scheduleScan();
	}

	[[nodiscard]] int count() const { return static_cast<int>(this->mEntries.size()); }
	[[nodiscard]] Status status() const { return this->mStatus; }

	Q_INVOKABLE [[nodiscard]] bool isFolder(int idx) const {
		if (idx < 0 || static_cast<std::size_t>(idx) >= this->mEntries.size()) return false;
		return this->mEntries[static_cast<std::size_t>(idx)].fileIsDir;
	}

	Q_INVOKABLE [[nodiscard]] QVariant get(int idx, const QString& property) const {
		if (idx < 0 || static_cast<std::size_t>(idx) >= this->mEntries.size()) return {};
		const auto& e = this->mEntries[static_cast<std::size_t>(idx)];
		if (property == QLatin1String("fileName")) return e.fileName;
		if (property == QLatin1String("filePath")) return e.filePath;
		if (property == QLatin1String("fileUrl") || property == QLatin1String("fileURL")) return e.fileUrl;
		if (property == QLatin1String("fileBaseName")) return e.fileBaseName;
		if (property == QLatin1String("fileSuffix")) return e.fileSuffix;
		if (property == QLatin1String("fileSize")) return e.fileSize;
		if (property == QLatin1String("fileModified")) return QDateTime::fromMSecsSinceEpoch(e.fileModifiedMs);
		if (property == QLatin1String("fileAccessed")) return QDateTime::fromMSecsSinceEpoch(e.fileAccessedMs);
		if (property == QLatin1String("fileIsDir")) return e.fileIsDir;
		return {};
	}

	Q_INVOKABLE [[nodiscard]] int indexOf(const QUrl& file) const {
		const QString target = QDir::cleanPath(urlToLocalPath(file));
		if (target.isEmpty()) return -1;
		for (std::size_t i = 0; i < this->mEntries.size(); ++i) {
			if (this->mEntries[i].filePath == target) return static_cast<int>(i);
		}
		return -1;
	}

	void onDirChanged() {
		if (!this->mComplete) return;
		this->mWatchDebounce.start();
	}

	void onScanDone(std::uint64_t gen) {
		auto raw = folder_take_scan(this->mId, gen);
		if (gen != this->mGen) return;
		std::vector<qsrs::folderlist::Entry> next;
		next.reserve(raw.size());
		for (const auto& d: raw) {
			QString path = qsrs::folderlist::qstr(d.file_path);
			next.push_back(qsrs::folderlist::Entry {
			    qsrs::folderlist::qstr(d.file_name),
			    path,
			    QUrl::fromLocalFile(path),
			    qsrs::folderlist::qstr(d.file_base_name),
			    qsrs::folderlist::qstr(d.file_suffix),
			    d.file_size,
			    d.file_modified_ms,
			    d.file_accessed_ms,
			    d.file_is_dir,
			});
		}
		const bool changed = (next != this->mEntries);
		if (changed) {
			const int oldCount = static_cast<int>(this->mEntries.size());
			const int newCount = static_cast<int>(next.size());
			this->beginResetModel();
			this->mEntries = std::move(next);
			this->endResetModel();
			if (oldCount != newCount) {
				emit this->countChanged();
				emit this->rowCountChanged();
			}
			// Notify `onStatusChanged` listeners (e.g. in-place renames with unchanged count)
			// even when a watch-triggered rescan stayed in Ready without flashing Loading.
			if (this->mStatus == Status::Ready) {
				emit this->statusChanged();
			} else {
				this->setStatus(Status::Ready);
			}
		} else {
			this->setStatus(Status::Ready);
		}
	}

signals:
	void folderChanged();
	void rowCountChanged();
	void rootFolderChanged();
	void nameFilterChanged();
	void sortFieldChanged();
	void sortReversedChanged();
	void showFilesChanged();
	void showDirsChanged();
	void showDirsFirstChanged();
	void showDotAndDotDotChanged();
	void showDotChanged();
	void showDotDotChanged();
	void showHiddenChanged();
	void showOnlyReadableChanged();
	void caseSensitiveChanged();
	void sortCaseSensitiveChanged();
	void countChanged();
	void statusChanged();

private:
	static QString urlToLocalPath(const QUrl& url) {
		if (url.isEmpty()) return {};
		if (url.isLocalFile()) return url.toLocalFile();
		if (url.scheme().isEmpty()) return url.toString();
		return {};
	}

	void setStatus(Status s) {
		if (this->mStatus == s) return;
		this->mStatus = s;
		emit this->statusChanged();
	}

	void syncWatch() {
		const auto utf8 = this->mFolderPath.toUtf8();
		folder_watch_set(this->mId, rust::Str(utf8.constData(), static_cast<std::size_t>(utf8.size())));
	}

	void scheduleScan() {
		if (!this->mComplete) return;
		this->mScanDebounce.start();
	}

	void startScan(bool fromWatch) {
		if (!this->mComplete || this->mFolderPath.isEmpty()) return;
		this->mScanDebounce.stop();
		this->mWatchDebounce.stop();
		++this->mGen;
		if (!fromWatch || this->mStatus != Status::Ready) {
			this->setStatus(Status::Loading);
		}

		FolderScanRequest req;
		req.folder = rust::String(this->mFolderPath.toStdString());
		for (const auto& f: this->mNameFilters) {
			req.name_filters.push_back(rust::String(f.toStdString()));
		}
		req.sort_field = static_cast<std::int32_t>(this->mSortField);
		req.sort_reversed = this->mSortReversed;
		req.show_files = this->mShowFiles;
		req.show_dirs = this->mShowDirs;
		req.show_dirs_first = this->mShowDirsFirst;
		req.show_dot = this->mShowDot;
		req.show_dot_dot = this->mShowDotDot;
		req.show_hidden = this->mShowHidden;
		req.show_only_readable = this->mShowOnlyReadable;
		req.case_sensitive = this->mCaseSensitive;
		req.sort_case_sensitive = this->mSortCaseSensitive;

		folder_scan_async(this->mId, this->mGen, std::move(req));
	}

	std::uint64_t mId = 0;
	std::uint64_t mGen = 0;
	bool mComplete = true;
	QUrl mFolder;
	QString mFolderPath;
	QUrl mRootFolder;
	QStringList mNameFilters = {QStringLiteral("*")};
	SortField mSortField = SortField::Name;
	bool mSortReversed = false;
	bool mShowFiles = true;
	bool mShowDirs = true;
	bool mShowDirsFirst = false;
	bool mShowDot = false;
	bool mShowDotDot = false;
	bool mShowHidden = false;
	bool mShowOnlyReadable = false;
	bool mCaseSensitive = true;
	bool mSortCaseSensitive = true;
	Status mStatus = Status::Null;
	std::vector<qsrs::folderlist::Entry> mEntries;
	QTimer mScanDebounce;
	QTimer mWatchDebounce;
};
