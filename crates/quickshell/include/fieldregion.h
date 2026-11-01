#pragma once
// FieldRegion (qs-rs only, feature "field-region"): the compositor blur region of an iRiS field, rasterised on every
// build from the distances inir's IrisField.frag draws (squircle bodies or a Kiri Design's eased cuts, the frame
// band with its music wave, polynomial joins). A pixel is held when its centre lies `inset` inside the
// silhouette. inir's QML walk of the same shape was coarse while anything moved and exact only 96 ms after rest; here
// each row is traced with the steps the distance allows, so it is exact on every frame.
#include <QtCore/QElapsedTimer>
#include <QtCore/QList>
#include <QtCore/QVariantMap>
#include <QtGui/QRegion>
#include <QtGui/QVector4D>
#include <QtQml/qqml.h>
#include <algorithm>
#include <cmath>
#include <vector>
#include "window.h"

class FieldRegion: public PendingRegion {
	Q_OBJECT;
	QML_NAMED_ELEMENT(FieldRegion);
	/// Eight numbers a body: x, y, width, height, radius (0 under a Kiri Design: its own), fuse, first
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
	/// A Kiri Design's cut (0: round bodies): bodies with no radius take cut / 1.5, joins at most 2 cut deep.
	Q_PROPERTY(qreal chamfer MEMBER mChamfer NOTIFY fieldChanged);
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
		QRegion region = this->raster();
		this->sMicros = int(timer.nsecsElapsed() / 1000);
		return region;
	}

signals:
	void fieldChanged();

private:
	static constexpr double FAR = 1e9;
	struct Body {
		double cx, cy, hx, hy;
		double r[4]; // upper left, upper right, lower right, lower left
		double k;
		int j0, j1;
		double top, bottom, left, right;
	};

	static double roundedBox(double px, double py, double cx, double cy, double hx, double hy, double radius) {
		double r = std::min(radius, std::min(hx, hy));
		double qx = std::abs(px - cx) - (hx - r), qy = std::abs(py - cy) - (hy - r);
		return std::hypot(std::max(qx, 0.0), std::max(qy, 0.0)) + std::min(std::max(qx, qy), 0.0) - r;
	}

	// IrisField.frag squircleBox: the quartic superellipse over 1.8409 r.
	static double squircleBox(double px, double py, double cx, double cy, double hx, double hy, double radius) {
		double m = std::min(hx, hy), r = std::min(radius, m);
		if (r < 0.5 || r >= m - 0.5) return roundedBox(px, py, cx, cy, hx, hy, radius);
		double reach = std::min(r * 1.8409, m);
		double qx = std::abs(px - cx) - (hx - reach), qy = std::abs(py - cy) - (hy - reach);
		if (qx <= 0.0 || qy <= 0.0) return std::hypot(std::max(qx, 0.0), std::max(qy, 0.0)) + std::min(std::max(qx, qy), 0.0) - reach;
		double x2 = qx * qx, y2 = qy * qy;
		double l = std::sqrt(std::sqrt(x2 * x2 + y2 * y2)), l3 = l * l * l;
		return (l - reach) / std::hypot(x2 * qx / l3, y2 * qy / l3);
	}

	// IrisField.frag kiriBox (a Kiri Design, AIRISU): a corner is round, or with `cut` cut at 45° by 1.5 radius with both
	// ends eased by 0.55 radius; the radius gives way to 0.56 of the shorter half.
	static double kiriBox(double px, double py, double cx, double cy, double hx, double hy, double radius, bool cut) {
		double dx = px - cx, dy = py - cy;
		double r = std::min(radius, 0.56 * std::min(hx, hy));
		if (!cut || r < 0.5) return squircleBox(px, py, cx, cy, hx, hy, r);
		double e = 0.55 * r, qx = std::abs(dx), qy = std::abs(dy), sx = hx - e, sy = hy - e;
		double l = hx + hy - 1.5 * r - e * 1.41421356;
		double inside = std::max(std::max(qx - sx, qy - sy), (qx + qy - l) * 0.70710678);
		if (inside <= 0.0) return inside - e;
		double ax = sx, ay = l - sx, bx = l - sy, by = sy, abx = bx - ax, aby = by - ay;
		double t = std::clamp(((qx - ax) * abx + (qy - ay) * aby) / (abx * abx + aby * aby), 0.0, 1.0);
		double dist = std::hypot(qx - ax - abx * t, qy - ay - aby * t);
		dist = std::min(dist, qy <= ay ? std::abs(qx - sx) : std::hypot(qx - ax, qy - ay));
		dist = std::min(dist, qx <= bx ? std::abs(qy - sy) : std::hypot(qx - bx, qy - by));
		return dist - e;
	}

	// IrisField.frag bodyBox: each quadrant is the box with its own corner's radius all round; a negative one is cut.
	[[nodiscard]] double bodyDistance(const Body& b, double px, double py) const {
		double lane = b.r[py < b.cy ? (px < b.cx ? 0 : 1) : (px < b.cx ? 3 : 2)];
		double r = std::abs(lane);
		if (this->mChamfer <= 0.0) return squircleBox(px, py, b.cx, b.cy, b.hx, b.hy, r);
		return kiriBox(px, py, b.cx, b.cy, b.hx, b.hy, r > 0.5 ? r : this->mChamfer / 1.5, lane < 0.0);
	}

	// How far the side bands swell at a row, 0..1 of their reach (IrisField.frag's alongY), kept for the row.
	[[nodiscard]] double sideSwell(double py) const {
		if (py != this->alongRow) {
			double t = this->mWaveClock;
			this->alongRow = py;
			this->alongY = 0.60 + 0.24 * std::sin(py * 0.011 + t) + 0.16 * std::sin(py * 0.027 - t * 1.4);
		}
		return this->alongY;
	}

	[[nodiscard]] double frameDistance(double px, double py) const {
		double w = this->mWidthF, h = this->mHeightF, band = this->mBand;
		double lx = band, ly = band, hx = w - band, hy = h - band;
		if (this->waving) {
			double t = this->mWaveClock;
			double alongY = this->sideSwell(py);
			double alongX = 0.60 + 0.24 * std::sin(px * 0.009 - t) + 0.16 * std::sin(px * 0.023 + t * 1.2);
			lx += this->mWave.x() * alongY;
			ly += this->mWave.y() * alongX;
			hx -= this->mWave.z() * alongY;
			hy -= this->mWave.w() * alongX;
		}
		double cx = (lx + hx) * 0.5, cy = (ly + hy) * 0.5;
		double ex = std::max((hx - lx) * 0.5, 0.0), ey = std::max((hy - ly) * 0.5, 0.0);
		if (this->mChamfer > 0.0) return -kiriBox(px, py, cx, cy, ex, ey, this->mCorner, (px - cx) * (py - cy) < 0.0);
		return -squircleBox(px, py, cx, cy, ex, ey, this->mCorner);
	}

	[[nodiscard]] double smoothUnion(double a, double b, double k) const {
		// A Kiri Design keeps its shoulders tight: the fillet at most twice its cut (IrisField.frag smoothUnion).
		if (this->mChamfer > 0.0) k = std::min(k, 2.0 * this->mChamfer);
		if (k <= 0.001) return std::min(a, b);
		double h = std::clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
		return b + (a - b) * h - k * h * (1.0 - h);
	}

	// The held field at a point: the band, every active body, each body's joins.
	double held(double px, double py, const std::vector<int>& active, std::vector<double>& dist) const {
		this->sEvals++;
		double frame = this->mFramed ? this->frameDistance(px, py) : FAR;
		double d = frame;
		// A body past this point's reach is not measured, but it still bounds the step: no edge of it lies nearer than
		// its box, or a trace from the band jumped into it.
		double beyond = FAR;
		for (int i: active) {
			const auto& b = this->parsed[i];
			if (b.left > px || b.right < px) {
				beyond = std::min(beyond, b.left > px ? b.left - px : px - b.right);
				continue;
			}
			dist[i] = this->bodyDistance(b, px, py);
			d = std::min(d, dist[i]);
		}
		for (int i: active) {
			if (dist[i] >= FAR) continue;
			const auto& b = this->parsed[i];
			for (int j: {b.j0, b.j1}) {
				if (j == -1 && this->mFramed && this->mJoinsFrame) d = std::min(d, this->smoothUnion(frame, dist[i], b.k));
				else if (j > 0 && j <= int(dist.size()) && dist[j - 1] < FAR) d = std::min(d, this->smoothUnion(dist[j - 1], dist[i], b.k));
			}
		}
		for (int i: active) dist[i] = FAR;
		return std::min(d, beyond);
	}

	[[nodiscard]] QRegion raster() const {
		this->sRects = 0;
		this->sRows = 0;
		this->sEvals = 0;
		int width = int(std::round(this->mWidthF)), height = int(std::round(this->mHeightF));
		if (width <= 0 || height <= 0) return {};
		this->waving = this->mFramed && (this->mWave.x() > 0.01f || this->mWave.y() > 0.01f || this->mWave.z() > 0.01f || this->mWave.w() > 0.01f);
		this->parsed.clear();
		double reachK = 0.0;
		for (qsizetype n = 0; n + 7 < this->mBodies.size(); n += 8) {
			const auto* v = this->mBodies.constData() + n;
			qsizetype at = n / 2;
			const auto* c = at + 3 < this->mCornerRadii.size() ? this->mCornerRadii.constData() + at : nullptr;
			Body b {v[0] + v[2] / 2, v[1] + v[3] / 2, v[2] / 2, v[3] / 2, {c ? c[0] : v[4], c ? c[1] : v[4], c ? c[2] : v[4], c ? c[3] : v[4]},
				std::max(0.0, v[5]), int(v[6]), int(v[7]), 0, 0, 0, 0};
			reachK = std::max(reachK, b.k);
			this->parsed.push_back(b);
		}
		auto count = int(this->parsed.size());
		// A join's fillet swells the union at most k past either body; every body joined to another one or to the
		// frame is traced that far out, and only where it is the trace consults it.
		double grow = reachK + 2.0;
		for (auto& b: this->parsed) {
			bool real = b.hx > 0.0 && b.hy > 0.0;
			b.top = real ? b.cy - b.hy - grow : FAR;
			b.bottom = real ? b.cy + b.hy + grow : -FAR;
			b.left = real ? b.cx - b.hx - grow : FAR;
			b.right = real ? b.cx + b.hx + grow : -FAR;
		}
		std::vector<double> dist(size_t(count), FAR);
		std::vector<int> active;
		std::vector<std::pair<int, int>> spans, previous;
		std::vector<QRect> rects;
		int bandTop = 0;
		auto flushBand = [&](int bottom) {
			for (auto& s: previous) rects.emplace_back(s.first, bandTop, s.second - s.first, bottom - bandTop);
		};
		const double inset = this->mInset;
		this->alongRow = -1;
		// Rows that cross only the band's straight sides, past the opening's corners (and past the top and bottom swell),
		// hold what lies outside the box's sides: still, one traced row stands for the rest; waving, each row is its
		// sides' swell, read straight off the distance (tracing all 1080 rows cost ~4.8 ms a frame while music played).
		// A Kiri opening's cut (1.5 r and its ease) reaches further along a side than its round corner's squircle.
		double corner = this->mChamfer > 0.0 ? this->mCorner * 2.1 : this->mCorner * 1.8409;
		double swellTop = this->waving ? this->mWave.y() : 0.0, swellBottom = this->waving ? this->mWave.w() : 0.0;
		int plainFrom = int(std::ceil(this->mBand + swellTop + corner + 2));
		int plainTo = int(std::floor(height - this->mBand - swellBottom - corner - 2));
		bool straight = !this->waving || (this->mWave.y() <= 0.01f && this->mWave.w() <= 0.01f);
		bool lastPlain = false;
		for (int y = 0; y < height; ++y) {
			double cy = y + 0.5;
			active.clear();
			for (int i = 0; i < count; ++i)
				if (this->parsed[i].top <= cy && this->parsed[i].bottom >= cy) active.push_back(i);
			bool plain = this->mFramed && active.empty() && y >= plainFrom && y < plainTo;
			if (plain && !this->waving && lastPlain) continue;
			lastPlain = plain;
			spans.clear();
			if (plain && this->waving) {
				double alongY = this->sideSwell(cy);
				double lx = this->mBand + this->mWave.x() * alongY, hx = this->mWidthF - this->mBand - this->mWave.z() * alongY;
				// Held where the centre lies `inset` outside a side: x + 0.5 < lx - inset, x + 0.5 > hx + inset.
				int a = std::clamp(int(std::ceil(lx - inset - 0.5)), 0, width);
				int b = std::clamp(int(std::floor(hx + inset - 0.5)) + 1, 0, width);
				if (a >= b) spans.emplace_back(0, width);
				else {
					if (a > 0) spans.emplace_back(0, a);
					if (b < width) spans.emplace_back(b, width);
				}
				if (spans != previous) {
					flushBand(y);
					previous = spans;
					bandTop = y;
				}
				continue;
			}
			auto trace = [&](int from, int to) {
				int x = from;
				while (x < to) {
					double e = this->held(x + 0.5, cy, active, dist) + inset;
					// Every distance here changes by at most ~1.5 px per pixel (a gusset, the wave), so no edge lies
					// within |e| / 1.5 of a point: the pixels skipped share its side.
					int step = std::abs(e) >= FAR / 2 ? to - x : std::max(1, int(std::abs(e) * 0.65));
					int end = std::min(to, x + step);
					if (e < 0.0) {
						if (!spans.empty() && spans.back().second == x) spans.back().second = end;
						else spans.emplace_back(x, end);
					}
					x = end;
				}
			};
			if (this->mFramed && straight) {
				// Between the opening's corners and away from every body the band is straight, so a row holds all of
				// that stretch or none of it: one distance answers it, only corners and bodies are traced.
				double swell = this->waving ? this->sideSwell(cy) : 0.0;
				double lx = this->mBand + this->mWave.x() * swell, hx = this->mWidthF - this->mBand - this->mWave.z() * swell;
				std::vector<std::pair<int, int>> ranges {
				    {0, std::clamp(int(std::ceil(lx + corner + 2)), 0, width)},
				    {std::clamp(int(std::floor(hx - corner - 2)), 0, width), width}};
				for (int i: active)
					ranges.emplace_back(std::clamp(int(std::floor(this->parsed[i].left)), 0, width), std::clamp(int(std::ceil(this->parsed[i].right)), 0, width));
				std::sort(ranges.begin(), ranges.end());
				int done = 0;
				for (auto& r: ranges) {
					if (r.second <= done) continue;
					int from = std::max(r.first, done);
					if (from > done && this->frameDistance((done + from) * 0.5, cy) + inset < 0.0) {
						if (!spans.empty() && spans.back().second == done) spans.back().second = from;
						else spans.emplace_back(done, from);
					}
					trace(from, r.second);
					done = r.second;
				}
			} else if (this->mFramed) trace(0, width);
			else if (!active.empty()) {
				std::vector<std::pair<int, int>> ranges;
				for (int i: active) {
					int a = std::max(0, int(std::floor(this->parsed[i].left))), b = std::min(width, int(std::ceil(this->parsed[i].right)));
					if (b > a) ranges.emplace_back(a, b);
				}
				std::sort(ranges.begin(), ranges.end());
				int a = -1, b = -1;
				for (auto& r: ranges) {
					if (r.first > b) {
						if (b > a) trace(a, b);
						a = r.first;
						b = r.second;
					} else b = std::max(b, r.second);
				}
				if (b > a) trace(a, b);
			}
			if (this->mFramed || !active.empty()) this->sRows++;
			if (spans != previous) {
				flushBand(y);
				previous = spans;
				bandTop = y;
			}
		}
		flushBand(height);
		this->sRects = int(rects.size());
		QRegion region;
		if (!rects.empty()) region.setRects(rects.data(), int(rects.size()));
		return region;
	}

	QList<qreal> mBodies;
	QList<qreal> mCornerRadii;
	bool mFramed = false;
	bool mJoinsFrame = true;
	qreal mWidthF = 0, mHeightF = 0, mBand = 0, mCorner = 0, mChamfer = 0, mInset = 0.5;
	QVector4D mWave;
	qreal mWaveClock = 0;
	mutable std::vector<Body> parsed;
	mutable bool waving = false;
	mutable double alongRow = -1, alongY = 0;
	mutable int sRects = 0, sRows = 0, sMicros = 0;
	mutable qint64 sEvals = 0;
};
