//! Beat clock, closed-form step grid with swing, and the step tracker that turns
//! clock positions into "fire step N" events exactly once. Ported from
//! `nomad/engine/clock.py` (the specification).
//!
//! Clocks are plain data: the tracker receives `&Clock` on every call plus a
//! `clock_id` that stands in for Python's object identity (a different id means
//! the source switched).

pub const STEPS: usize = 16;
pub const FAR: i64 = 1 << 30;
/// Beats: float tolerance when a position lands on a boundary.
pub const EPS: f64 = 1e-9;

/// CPython's float floor division (`float_floor_div`), so `step_at` matches exactly.
pub fn py_floordiv(vx: f64, wx: f64) -> f64 {
    let m = vx % wx;
    let mut div = (vx - m) / wx;
    if m != 0.0 && ((wx < 0.0) != (m < 0.0)) {
        div -= 1.0;
    }
    if div != 0.0 {
        let mut fd = div.floor();
        if div - fd > 0.5 {
            fd += 1.0;
        }
        fd
    } else {
        0.0_f64.copysign(vx / wx)
    }
}

#[derive(Clone, Debug)]
pub struct Clock {
    pub sr: f64,
    pub bpm: f64,
    pub pos: f64,
    pub running: bool,
}

impl Clock {
    pub fn new(sample_rate: f64) -> Clock {
        Clock {
            sr: sample_rate,
            bpm: 110.0,
            pos: 0.0,
            running: false,
        }
    }

    pub fn start(&mut self) {
        if self.running {
            return;
        }
        self.pos = 0.0;
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.pos = 0.0;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.stop()
        } else {
            self.start()
        }
    }

    pub fn advance(&mut self, frames: i64) {
        if self.running {
            self.pos += frames as f64 * self.bpm / (60.0 * self.sr);
        }
    }

    /// Frames until `pos` reaches `beat` (>= 1); `FAR` when stopped.
    pub fn frames_to(&self, beat: f64) -> i64 {
        if !self.running {
            return FAR;
        }
        (((beat - self.pos) * 60.0 * self.sr / self.bpm - 1e-6).ceil() as i64).max(1)
    }
}

/// Even steps last `division*(1+swing)` beats, odd ones `division*(1-swing)`.
#[derive(Clone, Debug)]
pub struct StepGrid {
    pub division: f64,
    pub length: usize,
    pub swing: f64,
}

impl StepGrid {
    pub fn new(division: f64, length: usize, swing: f64) -> StepGrid {
        StepGrid {
            division,
            length,
            swing,
        }
    }

    pub fn step_beats(&self, k: i64) -> f64 {
        self.division
            * if k.rem_euclid(2) == 0 {
                1.0 + self.swing
            } else {
                1.0 - self.swing
            }
    }

    pub fn step_start(&self, k: i64) -> f64 {
        k.div_euclid(2) as f64 * 2.0 * self.division
            + k.rem_euclid(2) as f64 * self.division * (1.0 + self.swing)
    }

    /// Absolute step at `pos` (within EPS of a boundary counts as past it).
    pub fn step_at(&self, pos: f64) -> i64 {
        let d2 = 2.0 * self.division;
        let p = pos + EPS;
        let pair = py_floordiv(p, d2) as i64;
        let first = p - pair as f64 * d2 < self.division * (1.0 + self.swing);
        2 * pair + if first { 0 } else { 1 }
    }

    pub fn next_boundary(&self, pos: f64) -> f64 {
        self.step_start(self.step_at(pos) + 1)
    }

    /// `k mod length`, never negative (length 0 counts as 1 instead of raising).
    pub fn pattern_step(&self, k: i64) -> usize {
        k.rem_euclid(self.length.max(1) as i64) as usize
    }
}

#[derive(Clone, Debug)]
pub struct StepTracker {
    last: i64,
    last_pos: f64,
    clock: Option<u64>,
    joined: bool,
}

impl Default for StepTracker {
    fn default() -> Self {
        StepTracker::new()
    }
}

impl StepTracker {
    pub fn new() -> StepTracker {
        StepTracker {
            last: -1,
            last_pos: 0.0,
            clock: None,
            joined: false,
        }
    }

    pub fn poll(&mut self, grid: &StepGrid, clock: &Clock, clock_id: u64) -> Option<usize> {
        if self.clock != Some(clock_id) {
            // Source switched: join mid-run, never restart the pattern. A step on the
            // very boundary was already played by the old clock.
            self.joined = self.clock.is_some();
            self.clock = Some(clock_id);
            self.last = -1;
        }
        if !clock.running {
            self.last = -1;
            self.last_pos = 0.0;
            self.joined = false;
            return None;
        }
        let k = grid.step_at(clock.pos);
        let restarted = clock.pos < self.last_pos - EPS; // the clock went back to the start
        self.last_pos = clock.pos;
        if k == self.last {
            return None;
        }
        if k < self.last && !restarted {
            return None; // grid changed under us (swing drag): never repeat a step
        }
        let fresh = self.last < 0 || restarted;
        let joined = std::mem::replace(&mut self.joined, false);
        self.last = k;
        if fresh && (joined || (clock.pos - grid.step_start(k)).abs() > EPS) {
            return None; // joined mid-step (or at a handover): wait for the next boundary
        }
        Some(grid.pattern_step(k))
    }

    pub fn frames_to_event(&self, grid: &StepGrid, clock: &Clock) -> i64 {
        clock.frames_to(grid.next_boundary(clock.pos))
    }
}
