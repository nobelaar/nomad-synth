//! PolyBLEP oscillator (sine, triangle, saw, square), ported from
//! `nomad/engine/oscillator.py`. The Python file is the specification: same
//! formulas, same order of operations, phase in cycles `[0, 1)`.
//!
//! Python's `%` (and numpy's `remainder`) is a floored modulo; here that is
//! `rem_euclid(1.0)`.

pub const MAX_INC: f64 = 0.499;
pub const MAX_BLOCK: usize = 2048;

const TWO_PI: f64 = 2.0 * std::f64::consts::PI;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Wave {
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Wave {
    pub fn from_index(i: i64) -> Option<Wave> {
        match i {
            0 => Some(Wave::Sine),
            1 => Some(Wave::Triangle),
            2 => Some(Wave::Saw),
            3 => Some(Wave::Square),
            _ => None,
        }
    }
}

/// PolyBLEP correction; `t` is the phase in [0, 1), `dt` the increment.
fn blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let y = (t - 1.0) / dt;
        y * y + y + y + 1.0
    } else {
        0.0
    }
}

/// Band-limited square at phase `t`, duty cycle `pw`.
fn square(t: f64, inc: f64, pw: f64) -> f64 {
    let mut sq = if t < pw { 1.0 } else { -1.0 };
    sq += blep(t, inc);
    let mut edge = t - pw;
    edge -= edge.floor();
    sq -= blep(edge, inc);
    sq
}

/// Scalar PolyBLEP for the samples around a hard-sync reset (`dt` > 0 here).
fn blep_at(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        return x + x - x * x - 1.0;
    }
    if t > 1.0 - dt {
        let y = (t - 1.0) / dt;
        return y * y + y + y + 1.0;
    }
    0.0
}

/// Ideal waveform value at one phase, mirroring the render formulas.
fn value_at(t: f64, wave: Wave, pw: f64) -> f64 {
    match wave {
        Wave::Sine => (TWO_PI * t).sin(),
        Wave::Triangle => 4.0 * t.min(1.0 - t) - 1.0,
        Wave::Saw => 2.0 * t - 1.0,
        Wave::Square => {
            if t < pw {
                1.0
            } else {
                -1.0
            }
        }
    }
}

/// BLEP term the waveform applies for its own edge at phase 0.
fn edge_at(t: f64, inc: f64, wave: Wave) -> f64 {
    match wave {
        Wave::Saw => -blep_at(t, inc),
        Wave::Square => blep_at(t, inc),
        _ => 0.0,
    }
}

pub struct Oscillator {
    pub wave: Wave,
    pub phase: f64,
    sample_rate: f64,
    t: Vec<f64>,
    /// (sample index after the reset, fractional position inside the interval).
    resets: Vec<(usize, f64)>,
    /// Last master sample of the previous synced block; `None` after a free block.
    last_master: Option<f64>,
}

impl Oscillator {
    /// Panics if `sample_rate` is not positive (construction only, never on the audio path).
    pub fn new(sample_rate: f64) -> Oscillator {
        assert!(sample_rate > 0.0, "sample_rate must be positive");
        Oscillator {
            wave: Wave::Saw,
            phase: 0.0,
            sample_rate,
            t: vec![0.0; MAX_BLOCK],
            resets: Vec::with_capacity(MAX_BLOCK),
            last_master: None,
        }
    }

    /// Add one block of the current wave into `out`, advance and return the phase.
    ///
    /// `sync_master_phase` carries the master's phase for these samples; the local
    /// phase restarts at every master wrap. If its length differs from `out`, nothing
    /// is written and the phase is returned unchanged (Python raises instead).
    ///
    /// Scratch is sized for `MAX_BLOCK`; a longer block grows it once (allocates).
    pub fn render_into(
        &mut self,
        out: &mut [f64],
        freq: f64,
        pw: f64,
        sync_master_phase: Option<&[f64]>,
    ) -> f64 {
        let n = out.len();
        if n == 0 || sync_master_phase.is_some_and(|m| m.len() != n) {
            return self.phase;
        }
        let sr = self.sample_rate;
        let inc = if freq.is_finite() {
            freq.abs().min(MAX_INC * sr) / sr
        } else {
            0.0
        };
        let pw = if pw.is_finite() {
            pw.clamp(0.05, 0.95)
        } else {
            0.5
        };
        if self.t.len() < n {
            self.t.resize(n, 0.0);
        }
        let phase = self.phase;
        self.resets.clear();
        match sync_master_phase {
            Some(master) if inc > 0.0 => {
                self.synced_ramp(n, inc, phase, master);
                self.sync_correction(out, inc, pw);
            }
            _ => {
                // A frozen oscillator has no edges of its own to reset.
                for (i, ti) in self.t[..n].iter_mut().enumerate() {
                    *ti = (i as f64 * inc + phase).rem_euclid(1.0);
                }
                self.last_master = None;
            }
        }
        let t = &self.t[..n];
        add_wave(out, t, self.wave, inc, pw);
        self.phase = if self.resets.is_empty() {
            (phase + n as f64 * inc).rem_euclid(1.0)
        } else {
            // After a reset the free-running phase is meaningless.
            (t[n - 1] + inc).rem_euclid(1.0)
        };
        self.phase
    }

    /// Phase ramp restarted at every wrap of the master (`_synced_ramp`).
    fn synced_ramp(&mut self, n: usize, inc: f64, phase: f64, master: &[f64]) {
        let mut prev = self.last_master.unwrap_or(master[0]);
        self.last_master = Some(master[n - 1]);
        for (i, &b) in master.iter().enumerate() {
            if b < prev {
                let a = prev;
                // The master ran from a to 1 + b and crossed 1.0 this far into the interval.
                let frac = ((1.0 - a) / (1.0 + b - a)).clamp(0.0, 1.0);
                self.resets.push((i, frac));
            }
            prev = b;
        }
        let t = &mut self.t[..n];
        if self.resets.is_empty() {
            for (i, ti) in t.iter_mut().enumerate() {
                *ti = (i as f64 * inc + phase).rem_euclid(1.0);
            }
            return;
        }
        // rst: where the last reset happened, in samples (running maximum).
        let mut rst = -phase / inc;
        let mut next = self.resets.iter().peekable();
        for (i, ti) in t.iter_mut().enumerate() {
            if let Some(&&(j, frac)) = next.peek()
                && j == i
            {
                rst = rst.max((i as f64 - 1.0) + frac);
                next.next();
            }
            *ti = ((i as f64 - rst) * inc).rem_euclid(1.0);
        }
    }

    /// Band-limit the resets with a PolyBLEP scaled by their position (`_sync_correction`).
    fn sync_correction(&self, out: &mut [f64], inc: f64, pw: f64) {
        let (t, wave) = (&self.t, self.wave);
        for &(i, f) in &self.resets {
            let t_new = t[i];
            // Sample -1 belongs to the previous block: one step behind the start phase.
            let t_old = if i > 0 {
                t[i - 1]
            } else {
                (self.phase - inc).rem_euclid(1.0)
            };
            let jump = 0.5 * (value_at(t_new, wave, pw) - value_at(t_old, wave, pw));
            out[i] += -jump * f * f - edge_at(t_new, inc, wave);
            if i > 0 {
                out[i - 1] += jump * (1.0 - f) * (1.0 - f) - edge_at(t_old, inc, wave);
            }
        }
    }
}

/// Add the waveform for the phases `t` into `out` (the render formulas of Python).
fn add_wave(out: &mut [f64], t: &[f64], wave: Wave, inc: f64, pw: f64) {
    let pairs = out.iter_mut().zip(t);
    match wave {
        Wave::Sine => pairs.for_each(|(o, &t)| *o += (t * TWO_PI).sin()),
        Wave::Triangle => pairs.for_each(|(o, &t)| {
            let mut phi = t.min(1.0 - t);
            phi *= 4.0;
            phi -= 1.0;
            *o += phi;
        }),
        Wave::Saw => pairs.for_each(|(o, &t)| {
            let mut saw = t + t;
            saw -= 1.0;
            saw -= blep(t, inc);
            *o += saw;
        }),
        Wave::Square => pairs.for_each(|(o, &t)| *o += square(t, inc, pw)),
    }
}
