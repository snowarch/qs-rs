#pragma once
// Small Qt helpers that cxx-qt-lib does not expose.
#include <QtCore/QVariant>

namespace qsrs {
inline QVariant null_variant() { return QVariant::fromValue(nullptr); }
} // namespace qsrs
