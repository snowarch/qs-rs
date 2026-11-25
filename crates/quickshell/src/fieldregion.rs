//! Rasterizer for `FieldRegion` (the compositor blur region of an iRiS field): squircle bodies,
//! the frame band with its music wave, and polynomial smooth-union joins. A pixel is held when its
//! centre lies `inset` inside the silhouette.
//!
//! Scratch buffers live in thread-local storage (GUI thread) and `out_rects` is reused by the C++
//! wrapper, so steady-state frames do zero heap allocations.

#[cxx::bridge(namespace = "qsrs::field")]
pub mod ffi {
    struct FieldInput<'a> {
        bodies: &'a [f64],
        corner_radii: &'a [f64],
        framed: bool,
        joins_frame: bool,
        width: f64,
        height: f64,
        band: f64,
        corner: f64,
        inset: f64,
        wave_x: f64,
        wave_y: f64,
        wave_z: f64,
        wave_w: f64,
        wave_clock: f64,
    }

    struct FieldStats {
        rects: i32,
        rows: i32,
        evals: i64,
    }

    extern "Rust" {
        fn raster_field(input: &FieldInput, out_rects: Pin<&mut CxxVector<i32>>) -> FieldStats;
    }
}

use core::pin::Pin;
use std::cell::RefCell;

const FAR: f64 = 1e9;

#[derive(Clone, Copy)]
struct Body {
    cx: f64,
    cy: f64,
    hx: f64,
    hy: f64,
    r: [f64; 4], // upper left, upper right, lower right, lower left
    k: f64,
    j0: i32,
    j1: i32,
    top: f64,
    bottom: f64,
    left: f64,
    right: f64,
}

#[derive(Default)]
struct Scratch {
    parsed: Vec<Body>,
    dist: Vec<f64>,
    active: Vec<usize>,
    spans: Vec<(i32, i32)>,
    previous: Vec<(i32, i32)>,
    ranges: Vec<(i32, i32)>,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::default());
}

#[inline(always)]
fn rounded_box(px: f64, py: f64, cx: f64, cy: f64, hx: f64, hy: f64, radius: f64) -> f64 {
    let r = radius.min(hx.min(hy));
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    if qx > 0.0 && qy > 0.0 {
        (qx * qx + qy * qy).sqrt() - r
    } else {
        qx.max(qy) - r
    }
}

/// IrisField.frag squircleBox: the quartic superellipse over 1.8409 r.
#[inline(always)]
fn squircle_box(px: f64, py: f64, cx: f64, cy: f64, hx: f64, hy: f64, radius: f64) -> f64 {
    let m = hx.min(hy);
    let r = radius.min(m);
    if r < 0.5 || r >= m - 0.5 {
        return rounded_box(px, py, cx, cy, hx, hy, radius);
    }
    let reach = (r * 1.8409).min(m);
    let qx = (px - cx).abs() - (hx - reach);
    let qy = (py - cy).abs() - (hy - reach);
    if qx <= 0.0 || qy <= 0.0 {
        return qx.max(qy) - reach;
    }
    let x2 = qx * qx;
    let y2 = qy * qy;
    let l = (x2 * x2 + y2 * y2).sqrt().sqrt();
    let l3 = l * l * l;
    let gx = x2 * qx / l3;
    let gy = y2 * qy / l3;
    (l - reach) / (gx * gx + gy * gy).sqrt()
}

/// IrisField.frag bodyBox: each quadrant is the box with its own corner's radius all round.
#[inline(always)]
fn body_distance(b: &Body, px: f64, py: f64) -> f64 {
    let idx = if py < b.cy {
        if px < b.cx { 0 } else { 1 }
    } else if px < b.cx {
        3
    } else {
        2
    };
    squircle_box(px, py, b.cx, b.cy, b.hx, b.hy, b.r[idx])
}

#[inline(always)]
fn smooth_union(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.001 {
        return a.min(b);
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

#[inline(always)]
fn side_swell_at(py: f64, t: f64) -> f64 {
    0.60 + 0.24 * (py * 0.011 + t).sin() + 0.16 * (py * 0.027 - t * 1.4).sin()
}

#[inline(always)]
fn frame_distance(input: &ffi::FieldInput, waving: bool, along_y: f64, px: f64, py: f64) -> f64 {
    let mut lx = input.band;
    let mut ly = input.band;
    let mut hx = input.width - input.band;
    let mut hy = input.height - input.band;
    if waving {
        let t = input.wave_clock;
        let along_x = 0.60 + 0.24 * (px * 0.009 - t).sin() + 0.16 * (px * 0.023 + t * 1.2).sin();
        lx += input.wave_x * along_y;
        ly += input.wave_y * along_x;
        hx -= input.wave_z * along_y;
        hy -= input.wave_w * along_x;
    }
    let cx = (lx + hx) * 0.5;
    let cy = (ly + hy) * 0.5;
    let ex = ((hx - lx) * 0.5).max(0.0);
    let ey = ((hy - ly) * 0.5).max(0.0);
    -squircle_box(px, py, cx, cy, ex, ey, input.corner)
}

#[inline(always)]
fn held(
    input: &ffi::FieldInput,
    waving: bool,
    along_y: f64,
    px: f64,
    py: f64,
    parsed: &[Body],
    active: &[usize],
    dist: &mut [f64],
    evals: &mut i64,
) -> f64 {
    *evals += 1;
    let frame = if input.framed { frame_distance(input, waving, along_y, px, py) } else { FAR };
    let mut d = frame;
    let mut beyond = FAR;
    for &i in active {
        let b = &parsed[i];
        if b.left > px {
            let gap = b.left - px;
            if gap < beyond {
                beyond = gap;
            }
            continue;
        }
        if b.right < px {
            let gap = px - b.right;
            if gap < beyond {
                beyond = gap;
            }
            continue;
        }
        let bd = body_distance(b, px, py);
        dist[i] = bd;
        if bd < d {
            d = bd;
        }
    }
    let count = dist.len() as i32;
    for &i in active {
        let di = dist[i];
        if di >= FAR {
            continue;
        }
        let b = &parsed[i];
        for j in [b.j0, b.j1] {
            if j == -1 && input.framed && input.joins_frame {
                let u = smooth_union(frame, di, b.k);
                if u < d {
                    d = u;
                }
            } else if j > 0 && j <= count {
                let dj = dist[(j - 1) as usize];
                if dj < FAR {
                    let u = smooth_union(dj, di, b.k);
                    if u < d {
                        d = u;
                    }
                }
            }
        }
    }
    for &i in active {
        dist[i] = FAR;
    }
    d.min(beyond)
}

#[inline(always)]
fn push_span(spans: &mut Vec<(i32, i32)>, from: i32, to: i32) {
    if let Some(last) = spans.last_mut()
        && last.1 == from
    {
        last.1 = to;
        return;
    }
    spans.push((from, to));
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn trace_range(
    input: &ffi::FieldInput,
    waving: bool,
    along_y: f64,
    cy: f64,
    inset: f64,
    from: i32,
    to: i32,
    parsed: &[Body],
    active: &[usize],
    dist: &mut [f64],
    spans: &mut Vec<(i32, i32)>,
    evals: &mut i64,
) {
    let mut x = from;
    while x < to {
        let e = held(input, waving, along_y, x as f64 + 0.5, cy, parsed, active, dist, evals) + inset;
        let ae = e.abs();
        let step = if ae >= FAR * 0.5 { to - x } else { ((ae * 0.65) as i32).max(1) };
        let end = (x + step).min(to);
        if e < 0.0 {
            push_span(spans, x, end);
        }
        x = end;
    }
}

fn raster_with_scratch<F: FnMut(i32, i32, i32, i32)>(
    input: &ffi::FieldInput,
    s: &mut Scratch,
    mut push_rect: F,
) -> ffi::FieldStats {
    let width = input.width.round() as i32;
    let height = input.height.round() as i32;
    if width <= 0 || height <= 0 {
        return ffi::FieldStats { rects: 0, rows: 0, evals: 0 };
    }

    let waving = input.framed
        && (input.wave_x > 0.01 || input.wave_y > 0.01 || input.wave_z > 0.01 || input.wave_w > 0.01);

    s.parsed.clear();
    let mut reach_k = 0.0_f64;
    let bodies = input.bodies;
    let corners = input.corner_radii;
    let mut n = 0usize;
    while n + 7 < bodies.len() {
        let at = n / 2;
        let r = if at + 3 < corners.len() {
            [corners[at], corners[at + 1], corners[at + 2], corners[at + 3]]
        } else {
            [bodies[n + 4]; 4]
        };
        let k = bodies[n + 5].max(0.0);
        if k > reach_k {
            reach_k = k;
        }
        s.parsed.push(Body {
            cx: bodies[n] + bodies[n + 2] * 0.5,
            cy: bodies[n + 1] + bodies[n + 3] * 0.5,
            hx: bodies[n + 2] * 0.5,
            hy: bodies[n + 3] * 0.5,
            r,
            k,
            j0: bodies[n + 6] as i32,
            j1: bodies[n + 7] as i32,
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        });
        n += 8;
    }

    let grow = reach_k + 2.0;
    for b in &mut s.parsed {
        let real = b.hx > 0.0 && b.hy > 0.0;
        b.top = if real { b.cy - b.hy - grow } else { FAR };
        b.bottom = if real { b.cy + b.hy + grow } else { -FAR };
        b.left = if real { b.cx - b.hx - grow } else { FAR };
        b.right = if real { b.cx + b.hx + grow } else { -FAR };
    }

    s.dist.clear();
    s.dist.resize(s.parsed.len(), FAR);
    s.previous.clear();

    let mut rect_count = 0i32;
    let mut rows = 0i32;
    let mut evals = 0i64;
    let mut band_top = 0i32;

    let inset = input.inset;
    let corner = input.corner * 1.8409;
    let swell_top = if waving { input.wave_y } else { 0.0 };
    let swell_bottom = if waving { input.wave_w } else { 0.0 };
    let plain_from = (input.band + swell_top + corner + 2.0).ceil() as i32;
    let plain_to = (height as f64 - input.band - swell_bottom - corner - 2.0).floor() as i32;
    let straight = !waving || (input.wave_y <= 0.01 && input.wave_w <= 0.01);
    let mut last_plain = false;

    for y in 0..height {
        let cy = y as f64 + 0.5;
        s.active.clear();
        for (i, b) in s.parsed.iter().enumerate() {
            if b.top <= cy && b.bottom >= cy {
                s.active.push(i);
            }
        }

        let plain = input.framed && s.active.is_empty() && y >= plain_from && y < plain_to;
        if plain && !waving && last_plain {
            continue;
        }
        last_plain = plain;

        let along_y = if waving { side_swell_at(cy, input.wave_clock) } else { 0.0 };
        s.spans.clear();

        if plain && waving {
            let lx = input.band + input.wave_x * along_y;
            let hx = input.width - input.band - input.wave_z * along_y;
            let a = ((lx - inset - 0.5).ceil() as i32).clamp(0, width);
            let b = ((hx + inset - 0.5).floor() as i32 + 1).clamp(0, width);
            if a >= b {
                s.spans.push((0, width));
            } else {
                if a > 0 {
                    s.spans.push((0, a));
                }
                if b < width {
                    s.spans.push((b, width));
                }
            }
            if s.spans != s.previous {
                for &(x0, x1) in &s.previous {
                    push_rect(x0, band_top, x1 - x0, y - band_top);
                    rect_count += 1;
                }
                std::mem::swap(&mut s.previous, &mut s.spans);
                band_top = y;
            }
            continue;
        }

        let parsed = &s.parsed;
        let active = &s.active;
        let dist = &mut s.dist;
        let spans = &mut s.spans;

        if input.framed && straight {
            let lx = input.band + input.wave_x * along_y;
            let hx = input.width - input.band - input.wave_z * along_y;
            s.ranges.clear();
            s.ranges.push((0, ((lx + corner + 2.0).ceil() as i32).clamp(0, width)));
            s.ranges.push((((hx - corner - 2.0).floor() as i32).clamp(0, width), width));
            for &i in active {
                s.ranges.push((
                    (parsed[i].left.floor() as i32).clamp(0, width),
                    (parsed[i].right.ceil() as i32).clamp(0, width),
                ));
            }
            s.ranges.sort_unstable();
            let mut done = 0i32;
            for i in 0..s.ranges.len() {
                let (r_first, r_second) = s.ranges[i];
                if r_second <= done {
                    continue;
                }
                let from = r_first.max(done);
                if from > done && frame_distance(input, waving, along_y, (done + from) as f64 * 0.5, cy) + inset < 0.0 {
                    push_span(spans, done, from);
                }
                trace_range(input, waving, along_y, cy, inset, from, r_second, parsed, active, dist, spans, &mut evals);
                done = r_second;
            }
        } else if input.framed {
            trace_range(input, waving, along_y, cy, inset, 0, width, parsed, active, dist, spans, &mut evals);
        } else if !active.is_empty() {
            s.ranges.clear();
            for &i in active {
                let a = (parsed[i].left.floor() as i32).max(0);
                let b = (parsed[i].right.ceil() as i32).min(width);
                if b > a {
                    s.ranges.push((a, b));
                }
            }
            s.ranges.sort_unstable();
            let mut a = -1i32;
            let mut b = -1i32;
            for i in 0..s.ranges.len() {
                let (r_first, r_second) = s.ranges[i];
                if r_first > b {
                    if b > a {
                        trace_range(input, waving, along_y, cy, inset, a, b, parsed, active, dist, spans, &mut evals);
                    }
                    a = r_first;
                    b = r_second;
                } else if r_second > b {
                    b = r_second;
                }
            }
            if b > a {
                trace_range(input, waving, along_y, cy, inset, a, b, parsed, active, dist, spans, &mut evals);
            }
        }

        if input.framed || !s.active.is_empty() {
            rows += 1;
        }
        if s.spans != s.previous {
            for &(x0, x1) in &s.previous {
                push_rect(x0, band_top, x1 - x0, y - band_top);
                rect_count += 1;
            }
            std::mem::swap(&mut s.previous, &mut s.spans);
            band_top = y;
        }
    }

    for &(x0, x1) in &s.previous {
        push_rect(x0, band_top, x1 - x0, height - band_top);
        rect_count += 1;
    }

    ffi::FieldStats { rects: rect_count, rows, evals }
}

pub fn raster_field(input: &ffi::FieldInput, mut out_rects: Pin<&mut cxx::CxxVector<i32>>) -> ffi::FieldStats {
    let mut push = |x: i32, y: i32, w: i32, h: i32| {
        out_rects.as_mut().push(x);
        out_rects.as_mut().push(y);
        out_rects.as_mut().push(w);
        out_rects.as_mut().push(h);
    };
    SCRATCH.with(|cell| {
        if let Ok(mut scratch) = cell.try_borrow_mut() {
            raster_with_scratch(input, &mut scratch, &mut push)
        } else {
            let mut scratch = Scratch::default();
            raster_with_scratch(input, &mut scratch, &mut push)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster_vec(input: &ffi::FieldInput) -> (Vec<(i32, i32, i32, i32)>, ffi::FieldStats) {
        let mut rects = Vec::new();
        let mut scratch = Scratch::default();
        let stats = raster_with_scratch(input, &mut scratch, |x, y, w, h| rects.push((x, y, w, h)));
        (rects, stats)
    }

    fn holds(rects: &[(i32, i32, i32, i32)], x: i32, y: i32) -> bool {
        rects.iter().any(|&(rx, ry, rw, rh)| x >= rx && x < rx + rw && y >= ry && y < ry + rh)
    }

    #[test]
    fn framed_with_fillet_and_corners() {
        let bodies = [150.0, 0.0, 100.0, 60.0, 0.0, 20.0, -1.0, 0.0];
        let input = ffi::FieldInput {
            bodies: &bodies,
            corner_radii: &[],
            framed: true,
            joins_frame: true,
            width: 400.0,
            height: 300.0,
            band: 12.0,
            corner: 20.0,
            inset: 0.5,
            wave_x: 0.0,
            wave_y: 0.0,
            wave_z: 0.0,
            wave_w: 0.0,
            wave_clock: 0.0,
        };
        let (rects, stats) = raster_vec(&input);
        assert!(stats.rects > 0);
        assert!(holds(&rects, 5, 150));
        assert!(holds(&rects, 10, 150));
        assert!(!holds(&rects, 11, 150));
        assert!(!holds(&rects, 200, 150));
        assert!(holds(&rects, 147, 13));
        assert!(!holds(&rects, 130, 20));
        assert!(holds(&rects, 200, 40));
    }

    #[test]
    fn rounded_and_squircle_match_hypot_identity() {
        let cases: [(f64, f64); 6] = [(-5.0, -3.0), (-5.0, 4.0), (4.0, -5.0), (0.0, 0.0), (3.0, 4.0), (12.5, 7.25)];
        for (qx, qy) in cases {
            let ref_val = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0);
            let fast = if qx > 0.0 && qy > 0.0 { (qx * qx + qy * qy).sqrt() } else { qx.max(qy) };
            assert!((ref_val - fast).abs() < 1e-12);
        }
    }
}
