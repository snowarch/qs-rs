//! Physics engine for `SpringValue`: closed-form Apple `Spring(duration:bounce:)` integrator,
//! cubic-bezier curve segments, and critically damped mid-curve retarget chase.
//!
//! Pure Rust with inline 4-element curve arrays (no heap allocation per curve or per kick).

#[cxx::bridge(namespace = "qsrs::spring")]
pub mod ffi {
    #[derive(Clone, Copy, Default)]
    struct SpringParams {
        response: f64,
        bounce: f64,
        has_curve: bool,
        c0: f64,
        c1: f64,
        c2: f64,
        c3: f64,
    }

    struct StepOutcome {
        value: f64,
        velocity: f64,
        value_changed: bool,
        velocity_changed: bool,
        settled: bool,
    }

    struct KickOutcome {
        value: f64,
        velocity: f64,
        value_changed: bool,
        velocity_changed: bool,
        jumped: bool,
        start_ticker: bool,
    }

    extern "Rust" {
        type SpringEngine;

        fn new_spring() -> Box<SpringEngine>;
        fn value(self: &SpringEngine) -> f64;
        fn velocity(self: &SpringEngine) -> f64;
        fn rising(self: &SpringEngine) -> bool;
        fn set_velocity(self: &mut SpringEngine, velocity: f64) -> bool;
        fn set_active(self: &mut SpringEngine, params: &SpringParams);
        fn complete(self: &mut SpringEngine, to: f64) -> bool;
        fn jump(self: &mut SpringEngine, to: f64) -> StepOutcome;
        #[allow(clippy::too_many_arguments)]
        fn kick(
            self: &mut SpringEngine,
            to: f64,
            animate: bool,
            running: bool,
            minimum: f64,
            epsilon: f64,
            emerge: &SpringParams,
            recede: &SpringParams,
            has_recede: bool,
        ) -> KickOutcome;
        fn step(self: &mut SpringEngine, dt: f64, to: f64, minimum: f64, epsilon: f64) -> StepOutcome;
    }
}

use core::f64::consts::TAU;

#[derive(Clone, Copy)]
struct Segment {
    delta: f64,
    age: f64,
    response: f64,
    curve: [f64; 4],
}

pub struct SpringEngine {
    value: f64,
    velocity: f64,
    target: f64,
    chase: f64,
    rising: bool,
    active: ffi::SpringParams,
    segments: Vec<Segment>,
}

pub fn new_spring() -> Box<SpringEngine> {
    Box::new(SpringEngine {
        value: 0.0,
        velocity: 0.0,
        target: 0.0,
        chase: 0.0,
        rising: true,
        active: ffi::SpringParams::default(),
        segments: Vec::new(),
    })
}

#[inline]
fn bezier(c: [f64; 4], x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * c[0];
    let bx = 3.0 * (c[2] - c[0]) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * c[1];
    let by = 3.0 * (c[3] - c[1]) - cy;
    let ay = 1.0 - cy - by;
    let mut t = x;
    for _ in 0..8 {
        let fx = ((ax * t + bx) * t + cx) * t - x;
        let d = (3.0 * ax * t + 2.0 * bx) * t + cx;
        if fx.abs() < 1e-5 || d.abs() < 1e-6 {
            break;
        }
        t = (t - fx / d).clamp(0.0, 1.0);
    }
    ((ay * t + by) * t + cy) * t
}

impl SpringEngine {
    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    pub fn rising(&self) -> bool {
        self.rising
    }

    pub fn set_velocity(&mut self, velocity: f64) -> bool {
        if velocity == self.velocity {
            return false;
        }
        self.velocity = velocity;
        true
    }

    pub fn set_active(&mut self, params: &ffi::SpringParams) {
        self.active = *params;
    }

    pub fn complete(&mut self, to: f64) -> bool {
        self.target = to;
        if to == self.value {
            return false;
        }
        self.value = to;
        true
    }

    pub fn jump(&mut self, to: f64) -> ffi::StepOutcome {
        self.segments.clear();
        self.chase = 0.0;
        let vel_changed = self.velocity != 0.0;
        self.velocity = 0.0;
        self.target = to;
        let val_changed = self.value != to;
        self.value = to;
        ffi::StepOutcome {
            value: self.value,
            velocity: self.velocity,
            value_changed: val_changed,
            velocity_changed: vel_changed,
            settled: true,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn kick(
        &mut self,
        to: f64,
        animate: bool,
        running: bool,
        _minimum: f64,
        epsilon: f64,
        emerge: &ffi::SpringParams,
        recede: &ffi::SpringParams,
        has_recede: bool,
    ) -> ffi::KickOutcome {
        self.rising = to >= self.value;
        let p = if !self.rising && has_recede { *recede } else { *emerge };
        if !animate || p.response <= 0.0 || ((self.value - to).abs() < epsilon && self.velocity.abs() < epsilon * 12.0) {
            let j = self.jump(to);
            return ffi::KickOutcome {
                value: j.value,
                velocity: j.velocity,
                value_changed: j.value_changed,
                velocity_changed: j.velocity_changed,
                jumped: true,
                start_ticker: false,
            };
        }
        if p.has_curve && running && (!self.segments.is_empty() || self.chase > 0.0) {
            self.segments.clear();
            self.chase = p.response;
        } else if p.has_curve {
            let delta = to - if self.segments.is_empty() { self.value } else { self.target };
            if delta.abs() >= epsilon {
                self.segments.push(Segment { delta, age: 0.0, response: p.response, curve: [p.c0, p.c1, p.c2, p.c3] });
            }
        } else {
            self.segments.clear();
        }
        self.target = to;
        self.active = p;
        ffi::KickOutcome {
            value: self.value,
            velocity: self.velocity,
            value_changed: false,
            velocity_changed: false,
            jumped: false,
            start_ticker: !running,
        }
    }

    pub fn step(&mut self, dt: f64, to: f64, minimum: f64, epsilon: f64) -> ffi::StepOutcome {
        let p = self.active;
        if p.response <= 0.0 {
            return self.jump(to);
        }

        if self.chase <= 0.0 && (!self.segments.is_empty() || p.has_curve) {
            let mut offset = 0.0;
            let mut write = 0;
            for read in 0..self.segments.len() {
                let mut seg = self.segments[read];
                seg.age += dt * 1000.0;
                if seg.age < seg.response {
                    offset += seg.delta * (1.0 - bezier(seg.curve, seg.age / seg.response));
                    self.segments[write] = seg;
                    write += 1;
                }
            }
            self.segments.truncate(write);
            if !self.segments.is_empty() || p.has_curve {
                if self.segments.is_empty() {
                    return self.jump(to);
                }
                let next = minimum.max(to - offset);
                let new_vel = (next - self.value) / dt;
                let vel_changed = new_vel != self.velocity;
                let val_changed = next != self.value;
                self.velocity = new_vel;
                self.value = next;
                return ffi::StepOutcome {
                    value: self.value,
                    velocity: self.velocity,
                    value_changed: val_changed,
                    velocity_changed: vel_changed,
                    settled: false,
                };
            }
            self.value = to - offset;
        }

        let resp = if self.chase > 0.0 { self.chase } else { p.response };
        let w = TAU / (resp / 1000.0);
        let zeta = if self.chase > 0.0 { 1.0 } else { 1.0 - p.bounce };
        let x0 = self.value - to;
        let v0 = self.velocity;
        let (x, mut v) = if zeta < 0.9999 {
            let wd = w * (1.0 - zeta * zeta).sqrt();
            let e = (-zeta * w * dt).exp();
            let b = (v0 + zeta * w * x0) / wd;
            let (s, c) = (wd * dt).sin_cos();
            let x = e * (x0 * c + b * s);
            let v = -zeta * w * x + e * (-x0 * wd * s + b * wd * c);
            (x, v)
        } else {
            let e = (-w * dt).exp();
            let b = v0 + w * x0;
            let x = (x0 + b * dt) * e;
            let v = (b - w * (x0 + b * dt)) * e;
            (x, v)
        };
        let mut next = to + x;
        if next < minimum {
            next = minimum;
            v = 0.0;
        }
        if x.abs() < epsilon && v.abs() < epsilon * 12.0 {
            return self.jump(to);
        }
        let vel_changed = v != self.velocity;
        let val_changed = next != self.value;
        self.velocity = v;
        self.value = next;
        ffi::StepOutcome {
            value: self.value,
            velocity: self.velocity,
            value_changed: val_changed,
            velocity_changed: vel_changed,
            settled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(response: f64, bounce: f64, curve: Option<[f64; 4]>) -> ffi::SpringParams {
        match curve {
            Some([c0, c1, c2, c3]) => ffi::SpringParams { response, bounce, has_curve: true, c0, c1, c2, c3 },
            None => ffi::SpringParams { response, bounce, ..Default::default() },
        }
    }

    #[test]
    fn critically_damped_settles_monotonically() {
        let mut s = new_spring();
        let p = params(300.0, 0.0, None);
        let k = s.kick(1.0, true, false, -1e9, 0.0005, &p, &ffi::SpringParams::default(), false);
        assert!(k.start_ticker);
        let mut prev = 0.0;
        let mut settled = false;
        for _ in 0..100 {
            let r = s.step(1.0 / 60.0, 1.0, -1e9, 0.0005);
            assert!(r.value >= prev - 1e-12);
            prev = r.value;
            if r.settled {
                settled = true;
                break;
            }
        }
        assert!(settled);
        assert_eq!(s.value(), 1.0);
        assert_eq!(s.velocity(), 0.0);
    }

    #[test]
    fn underdamped_overshoots_and_retarget_keeps_velocity() {
        let mut s = new_spring();
        let p = params(300.0, 0.4, None);
        s.kick(1.0, true, false, -1e9, 0.0005, &p, &ffi::SpringParams::default(), false);
        let mut peak = 0.0_f64;
        for _ in 0..120 {
            let r = s.step(1.0 / 60.0, 1.0, -1e9, 0.0005);
            peak = peak.max(r.value);
            if r.settled {
                break;
            }
        }
        assert!(peak > 1.05, "expected overshoot, got {peak}");

        // Retarget mid-flight preserves momentum.
        let mut s2 = new_spring();
        let crit = params(400.0, 0.0, None);
        s2.kick(1.0, true, false, -1e9, 0.0005, &crit, &ffi::SpringParams::default(), false);
        for _ in 0..6 {
            s2.step(1.0 / 60.0, 1.0, -1e9, 0.0005);
        }
        let at_retarget = s2.value();
        s2.kick(0.0, true, true, -1e9, 0.0005, &crit, &ffi::SpringParams::default(), false);
        let next = s2.step(1.0 / 60.0, 0.0, -1e9, 0.0005);
        assert!(next.value > at_retarget, "velocity was not preserved");
    }

    #[test]
    fn curve_and_mid_curve_chase() {
        let mut s = new_spring();
        let p = params(200.0, 0.0, Some([0.3, 0.0, 0.2, 1.0]));
        s.kick(1.0, true, false, -1e9, 0.0005, &p, &ffi::SpringParams::default(), false);
        let mut steps = 0;
        while steps < 30 {
            steps += 1;
            if s.step(1.0 / 60.0, 1.0, -1e9, 0.0005).settled {
                break;
            }
        }
        assert_eq!(s.value(), 1.0);
        assert!((11..=14).contains(&steps));

        // Mid-curve retarget chases smoothly without snapping.
        let mut s2 = new_spring();
        let p2 = params(300.0, 0.0, Some([0.05, 0.7, 0.1, 1.0]));
        s2.kick(2.0, true, false, -1e9, 0.0005, &p2, &ffi::SpringParams::default(), false);
        for _ in 0..5 {
            s2.step(1.0 / 60.0, 2.0, -1e9, 0.0005);
        }
        let before = s2.value();
        s2.kick(0.0, true, true, -1e9, 0.0005, &p2, &ffi::SpringParams::default(), false);
        let r = s2.step(1.0 / 60.0, 0.0, -1e9, 0.0005);
        assert!((r.value - before).abs() < 0.5);
    }
}
