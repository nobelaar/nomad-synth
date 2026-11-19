//! Low-frequency oscillator (sine, triangle, saw, square, sample & hold),
//! ported from `nomad/engine/lfo.py`. Sample & hold draws from [`NumpyRng`],
//! so a seed gives the same sequence as Python.

use crate::rng::NumpyRng;

const TWO_PI: f64 = 2.0 * std::f64::consts::PI;
const NUM_WAVES: i64 = 5;

#[derive(Debug, PartialEq)]
pub struct InvalidWave;

/// Python's `_finite`: non-finite (including +inf) -> `lo`, else clamped.
fn finite(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_finite() { x.max(lo).min(hi) } else { lo }
}

pub struct Lfo {
    /// Base phase in cycles, `[0, 1)`.
    pub phase: f64,
    rng: NumpyRng,
    rate: f64,
    wave: i64,
    depth: f64,
    offset: f64,
    held: f64,
}

impl Lfo {
    /// `sample_rate` is unused (the LFO is time based), kept for symmetry with Python.
    pub fn new(sample_rate: f64, seed: u64) -> Lfo {
        let _ = sample_rate;
        let mut lfo = Lfo {
            phase: 0.0,
            rng: NumpyRng::new(seed),
            rate: 1.0,
            wave: 0,
            depth: 1.0,
            offset: 0.0,
            held: 0.0,
        };
        lfo.held = lfo.draw();
        lfo
    }

    fn draw(&mut self) -> f64 {
        self.rng.random() * 2.0 - 1.0
    }

    /// `wave` outside 0..=4 returns `Err` and changes nothing.
    pub fn set(
        &mut self,
        rate_hz: f64,
        wave: i64,
        depth: f64,
        phase_offset: f64,
    ) -> Result<(), InvalidWave> {
        if !(0..NUM_WAVES).contains(&wave) {
            return Err(InvalidWave);
        }
        self.wave = wave;
        self.rate = finite(rate_hz, 0.0, f64::INFINITY);
        self.depth = finite(depth, 0.0, 1.0);
        self.offset = if phase_offset.is_finite() {
            phase_offset.rem_euclid(1.0)
        } else {
            0.0
        };
        Ok(())
    }

    /// Advance by `seconds` and return the new value, `|v| <= depth`.
    pub fn advance(&mut self, seconds: f64) -> f64 {
        let dt = finite(seconds, 0.0, f64::INFINITY);
        let mut step = self.rate * dt;
        if !step.is_finite() {
            step = 0.0;
        }
        let mut new = self.phase + step;
        if new >= 1.0 {
            new = new.rem_euclid(1.0);
            if self.wave == 4 {
                self.held = self.draw();
            }
        }
        self.phase = new;
        let p = (new + self.offset).rem_euclid(1.0);
        let v = match self.wave {
            0 => (TWO_PI * p).sin(),
            1 => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.held,
        };
        v * self.depth
    }
}
