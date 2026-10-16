#pragma once
// Qt message handler feeding the instance logger (src/log.rs), installed before the app exists
// so nothing Qt prints is missed (upstream LogManager::init).
#include <QtCore/QtLogging>
#include <QtCore/QString>

#include "quickshell/src/log.cxx.h"

namespace qsrs {

inline void install_message_handler() {
	qInstallMessageHandler([](QtMsgType type, const QMessageLogContext& context, const QString& msg) {
		auto body = msg.toUtf8();
		const char* category = context.category != nullptr ? context.category : "default";
		log_qt_message(
		    static_cast<int32_t>(type),
		    rust::Str(category),
		    rust::Str(body.constData(), static_cast<size_t>(body.size()))
		);
	});
}

} // namespace qsrs
