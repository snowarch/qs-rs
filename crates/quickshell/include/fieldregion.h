#pragma once
// FieldRegion (qs-rs only, feature "field-region"): the compositor blur region of an iRiS field,
// rasterised in Rust (src/fieldregion.rs) from the distances inir's IrisField.frag draws
// (squircle bodies, the frame band with its music wave, polynomial joins).
#include <QtCore/QElapsedTimer>
#include <QtCore/QList>
#include <QtCore/QVariantMap>
#include <QtGui/QRegion>
#include <QtGui/QVector4D>
#include <QtQml/qqml.h>
#include <cmath>
#include <vector>
#include "window.h"
#include "quickshell/src/fieldregion.cxx.h"

class FieldRegion: public PendingRegion {
	Q_OBJECT;
	QML_NAMED_ELEMENT(FieldRegion);
	/// Eight numbers a body: x, y, width, height, radius, fuse, first
	/// and second join (0 none, -1 the frame, n the body at n - 1).
	Q_PROPERTY(QList<qreal> bodies READ bodies WRITE setBodies NOTIFY fieldChanged);
	/// Four radii a body (upper left, upper right, lower right, lower left), body by body as in `bodies`: a menu nests the
	/// corner by a frame corner or a fillet and keeps its own at the others. Left short, a body's corners take its radius.
	Q_PROPERTY(QList<qreal> cornerRadii READ cornerRadii WRITE setCornerRadii NOTIFY fieldChanged);
	Q_PROPERTY(bool framed MEMBER mFramed NOTIFY fieldChanged);
	Q_PROPERTY(bool joinsFrame MEMBER mJoinsFrame NOTIFY fieldChanged);
	Q_PROPERTY(qreal windowWidth MEMBER mWidthF NOTIFY fieldChanged);
	Q_PROPERTY(qreal windowHeight MEMBER mHeightF NOTIFY fieldChanged);
	Q_PROPERTY(qreal band MEMBER mBand NOTIFY fieldChanged);
	Q_PROPERTY(qreal cornerRadius MEMBER mCorner NOTIFY fieldChanged);
	Q_PROPERTY(qreal inset MEMBER mInset NOTIFY fieldChanged);
	/// How far the band's inner line swells on each side (left, top, right, bottom) and the swell's phase.
	Q_PROPERTY(QVector4D wave MEMBER mWave NOTIFY fieldChanged);
	Q_PROPERTY(qreal waveClock MEMBER mWaveClock NOTIFY fieldChanged);

public:
	explicit FieldRegion(QObject* parent = nullptr): PendingRegion(parent) {
		QObject::connect(this, &FieldRegion::fieldChanged, this, &PendingRegion::changed);
	}

	[[nodiscard]] QList<qreal> bodies() const { return this->mBodies; }
	[[nodiscard]] QList<qreal> cornerRadii() const { return this->mCornerRadii; }
	void setCornerRadii(const QList<qreal>& corners) {
		if (corners == this->mCornerRadii) return;
		this->mCornerRadii = corners;
		emit this->fieldChanged();
	}
	void setBodies(const QList<qreal>& bodies) {
		if (bodies == this->mBodies) return;
		this->mBodies = bodies;
		emit this->fieldChanged();
	}

	/// The last build: rects sent, rows traced, distance evaluations and microseconds.
	Q_INVOKABLE QVariantMap stats() const {
		return {{"rects", this->sRects}, {"rows", this->sRows}, {"evaluations", this->sEvals}, {"micros", this->sMicros}};
	}

	/// Whether the pixel at (x, y) is in the region a build gives now (tests).
	Q_INVOKABLE bool holds(int x, int y) const { return this->build().contains(QPoint(x, y)); }

	/// One row of that region, '1' held and '0' free (tests).
	Q_INVOKABLE QString row(int y) const {
		QRegion region = this->build();
		QString out(int(std::round(this->mWidthF)), QChar('0'));
		for (int x = 0; x < out.size(); ++x)
			if (region.contains(QPoint(x, y))) out[x] = QChar('1');
		return out;
	}

	[[nodiscard]] QRegion build() const override {
		QElapsedTimer timer;
		timer.start();
		this->rawRects.clear();
		qsrs::field::FieldInput input {
		    rust::Slice<const double>(this->mBodies.constData(), size_t(this->mBodies.size())),
		    rust::Slice<const double>(this->mCornerRadii.constData(), size_t(this->mCornerRadii.size())),
		    this->mFramed,
		    this->mJoinsFrame,
		    this->mWidthF,
		    this->mHeightF,
		    this->mBand,
		    this->mCorner,
		    this->mInset,
		    double(this->mWave.x()),
		    double(this->mWave.y()),
		    double(this->mWave.z()),
		    double(this->mWave.w()),
		    this->mWaveClock,
		};
		auto st = qsrs::field::raster_field(input, this->rawRects);
		this->sRects = st.rects;
		this->sRows = st.rows;
		this->sEvals = st.evals;
		this->qRects.clear();
		const size_t n = this->rawRects.size() / 4;
		this->qRects.reserve(n);
		for (size_t i = 0; i < n; ++i) {
			const int* p = this->rawRects.data() + i * 4;
			this->qRects.emplace_back(p[0], p[1], p[2], p[3]);
		}
		QRegion region;
		if (!this->qRects.empty()) region.setRects(this->qRects.data(), int(this->qRects.size()));
		this->sMicros = int(timer.nsecsElapsed() / 1000);
		return region;
	}

signals:
	void fieldChanged();

private:
	QList<qreal> mBodies;
	QList<qreal> mCornerRadii;
	bool mFramed = false;
	bool mJoinsFrame = true;
	qreal mWidthF = 0, mHeightF = 0, mBand = 0, mCorner = 0, mInset = 0.5;
	QVector4D mWave;
	qreal mWaveClock = 0;
	mutable std::vector<int32_t> rawRects;
	mutable std::vector<QRect> qRects;
	mutable int sRects = 0, sRows = 0, sMicros = 0;
	mutable qint64 sEvals = 0;
};
