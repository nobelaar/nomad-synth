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

pub struct Oscillator {
    pub wave: Wave,
    pub phase: f64,
    sample_rate: f64,
    t: Vec<f64>,
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
        }
    }

    /// Add one block of the current wave into `out`, advance and return the phase.
    ///
    /// Scratch is sized for `MAX_BLOCK`; a longer block grows it once (allocates).
    pub fn render_into(
        &mut self,
        out: &mut [f64],
        freq: f64,
        pw: f64,
        sync_master_phase: Option<&[f64]>,
    ) -> f64 {
        let _ = sync_master_phase; // hard sync: Task 3
        let n = out.len();
        if n == 0 {
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
        let t = &mut self.t[..n];
        for (i, ti) in t.iter_mut().enumerate() {
            *ti = (i as f64 * inc + phase).rem_euclid(1.0);
        }
        add_wave(out, t, self.wave, inc, pw);
        self.phase = (phase + n as f64 * inc).rem_euclid(1.0);
        self.phase
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
