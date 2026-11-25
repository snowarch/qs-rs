#include "folderlistmodel.h"

#include <QtCore/QFile>
#include <utility>

namespace qsrs::folderlist {

QHash<std::uint64_t, QsFolderListModel*>& registry() {
	static QHash<std::uint64_t, QsFolderListModel*> map;
	return map;
}

} // namespace qsrs::folderlist

extern "C" void qsrs_folderlist_dir_changed(std::uint64_t id) {
	if (auto* app = QCoreApplication::instance()) {
		QMetaObject::invokeMethod(
		    app,
		    [id] {
			    if (auto* m = qsrs::folderlist::registry().value(id, nullptr)) {
				    m->onDirChanged();
			    }
		    },
		    Qt::QueuedConnection);
	}
}

extern "C" void qsrs_folderlist_scan_done(std::uint64_t id, std::uint64_t gen) {
	if (auto* app = QCoreApplication::instance()) {
		QMetaObject::invokeMethod(
		    app,
		    [id, gen] {
			    if (auto* m = qsrs::folderlist::registry().value(id, nullptr)) {
				    m->onScanDone(gen);
			    } else {
				    // Model was destroyed before the scan arrived; discard stored results.
				    (void)folder_take_scan(id, gen);
			    }
		    },
		    Qt::QueuedConnection);
	}
}

extern "C" void qsrs_register_folder_list_model(const char* import_path_utf8) {
	if (qEnvironmentVariableIsSet("QSRS_QT_FOLDERLIST")) {
		if (import_path_utf8 != nullptr && import_path_utf8[0] != '\0') {
			QFile::remove(QString::fromUtf8(import_path_utf8) + QStringLiteral("/Qt/labs/folderlistmodel/qmldir"));
		}
		return;
	}
	if (import_path_utf8 != nullptr && import_path_utf8[0] != '\0') {
		const QString dirPath = QString::fromUtf8(import_path_utf8) + QStringLiteral("/Qt/labs/folderlistmodel");
		QDir().mkpath(dirPath);
		const QString qmldirPath = dirPath + QStringLiteral("/qmldir");
		static const QByteArray content("module Qt.labs.folderlistmodel\n");
		QFile existing(qmldirPath);
		if (!existing.open(QIODevice::ReadOnly) || existing.readAll() != content) {
			QFile out(qmldirPath);
			if (out.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
				out.write(content);
			}
		}
	}
	static bool done = false;
	if (done) return;
	done = true;
	for (auto [maj, min]: std::initializer_list<std::pair<int, int>>{
	         {1, 0}, {2, 1}, {2, 2}, {2, 11}, {2, 12}, {2, 15}, {6, 0}, {6, 4}, {6, 12}}) {
		qmlRegisterType<QsFolderListModel>("Qt.labs.folderlistmodel", maj, min, "FolderListModel");
	}
	qmlRegisterModule("Qt.labs.folderlistmodel", 6, 12);
}
