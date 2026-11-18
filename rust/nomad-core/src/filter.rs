//! 4-pole ladder filter with closed-form coefficients, ported from
//! `nomad/engine/filter.py` and the `ladder_coeffs_into`/`ladder_run` kernels.
//!
//! Deliberate difference from Python: `process` accepts any length (Python
//! rejects grains over 64). The state clamp still runs once per call, so the
//! caller keeps feeding grains of `MAX_GRAIN` (16 when k > 3.2) like the engine.

pub const K_MAX: f64 = 4.3;
pub const MAX_GRAIN: usize = 64;
const K_CLAMP: f64 = 3.2;
const ZI_LIMIT: f64 = 0.8;
const MIN_FC: f64 = 5.0;
const BINOM: [f64; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];

/// Closed-form ladder coefficients `(b, a)` for cutoff `fc_hz` and feedback `k`.
pub fn ladder_coeffs(fc_hz: f64, k: f64, sample_rate: f64) -> ([f64; 5], [f64; 5]) {
    let g = (std::f64::consts::PI * fc_hz / sample_rate).tan();
    let (pa, pb) = (1.0 + g, g - 1.0);
    let g4 = g.powi(4);
    let a0 = pa.powi(4) + k * g4;
    let (mut b, mut a) = ([0.0; 5], [0.0; 5]);
    for j in 0..5 {
        let c = BINOM[j];
        a[j] = (c * pa.powi(4 - j as i32) * pb.powi(j as i32) + k * g4 * c) / a0;
        b[j] = g4 * c / a0;
    }
    (b, a)
}

pub struct Ladder4Pole {
    sr: f64,
    zi: [f64; 4],
    b: [f64; 5],
    a: [f64; 5],
    k: f64,
    key: Option<(f64, f64)>,
}

impl Ladder4Pole {
    pub fn new(sample_rate: f64) -> Ladder4Pole {
        let mut f = Ladder4Pole {
            sr: sample_rate,
            zi: [0.0; 4],
            b: [0.0; 5],
            a: [0.0; 5],
            k: 0.0,
            key: None,
        };
        f.set(1000.0, 0.0);
        f
    }

    /// Cutoff `NaN` -> `0.45*sr`, resonance `NaN` -> 0; both clamped; cached on `(fc, r)`.
    pub fn set(&mut self, fc_hz: f64, resonance: f64) {
        let top = 0.45 * self.sr;
        let fc = if fc_hz.is_nan() {
            top
        } else {
            fc_hz.max(MIN_FC).min(top)
        };
        let r = if resonance.is_nan() {
            0.0
        } else {
            resonance.clamp(0.0, 1.0)
        };
        if self.key == Some((fc, r)) {
            return;
        }
        self.key = Some((fc, r));
        self.k = r * K_MAX;
        (self.b, self.a) = ladder_coeffs(fc, self.k, self.sr);
    }

    pub fn reset(&mut self) {
        self.zi = [0.0; 4];
    }

    /// Filter `buf` in place (DF-II transposed), then limit the state if k > 3.2.
    pub fn process(&mut self, buf: &mut [f64]) {
        let (b, a, zi) = (&self.b, &self.a, &mut self.zi);
        for x in buf.iter_mut() {
            let xn = *x;
            let y = b[0] * xn + zi[0];
            zi[0] = b[1] * xn + zi[1] - a[1] * y;
            zi[1] = b[2] * xn + zi[2] - a[2] * y;
            zi[2] = b[3] * xn + zi[3] - a[3] * y;
            zi[3] = b[4] * xn - a[4] * y;
            *x = y;
        }
        if self.k > K_CLAMP {
            let m = zi.iter().fold(0.0_f64, |m, z| m.max(z.abs()));
            if m > ZI_LIMIT {
                zi.iter_mut().for_each(|z| *z *= ZI_LIMIT / m);
            }
        }
    }
}
