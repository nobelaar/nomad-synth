//! Synthesized drum one-shots, ported from `render_one_shot` and its helpers in
//! `nomad/engine/drums.py` (the specification), plus the two scipy pieces they
//! need: a first-order Butterworth band-pass and `lfilter`.
//!
//! This allocates and is not for the audio thread: the drum machine receives the
//! finished shots through `set_bank`.

use crate::params::{DRUM_PARAMS, INSTRUMENTS, find};
use crate::rng::NumpyRng;

const TWO_PI: f64 = 2.0 * std::f64::consts::PI;
const MAX_SECONDS: f64 = 1.5;
const TAIL_FADE_S: f64 = 0.01;
const PEAK: f64 = 0.9;
/// Noise seed per instrument (kick..ohat), as `_SEED` in Python.
const SEED: [u64; 6] = [1, 2, 3, 4, 5, 6];

#[derive(Clone, Copy)]
struct C(f64, f64);

impl C {
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn div(self, o: C) -> C {
        let d = o.0 * o.0 + o.1 * o.1;
        C(
            (self.0 * o.0 + self.1 * o.1) / d,
            (self.1 * o.0 - self.0 * o.1) / d,
        )
    }
    /// Principal square root of a number with zero imaginary part.
    fn sqrt_real(x: f64) -> C {
        if x >= 0.0 {
            C(x.sqrt(), 0.0)
        } else {
            C(0.0, (-x).sqrt())
        }
    }
}

/// `scipy.signal.butter(1, [low, high], "bandpass")` with frequencies normalized
/// to Nyquist: pre-warp, analog prototype, `lp2bp_zpk`, `bilinear_zpk`, `zpk2tf`.
pub fn butter1_bandpass(low: f64, high: f64) -> ([f64; 3], [f64; 3]) {
    let fs = 2.0;
    let w1 = 2.0 * fs * (std::f64::consts::PI * low / fs).tan();
    let w2 = 2.0 * fs * (std::f64::consts::PI * high / fs).tan();
    let (bw, wo) = (w2 - w1, (w1 * w2).sqrt());
    // Prototype pole -1, scaled to the bandwidth and shifted to +-wo.
    let p_lp = C(-bw / 2.0, 0.0);
    let s = C::sqrt_real(p_lp.0 * p_lp.0 - wo * wo);
    let (p1, p2) = (p_lp.add(s), p_lp.sub(s));
    let k = bw; // k * bw**degree, degree 1
    // Bilinear transform: zero at the origin -> 1, extra zero at -1.
    let fs2 = C(2.0 * fs, 0.0);
    let (z1, z2) = (fs2.add(p1).div(fs2.sub(p1)), fs2.add(p2).div(fs2.sub(p2)));
    let kz = k * C(2.0 * fs, 0.0).div(fs2.sub(p1).mul(fs2.sub(p2))).0;
    let b = [kz, 0.0, -kz];
    let a = [1.0, -(z1.0 + z2.0), z1.mul(z2).0];
    (b, a)
}

/// `scipy.signal.lfilter(b, a, x)` in place: direct form II transposed, zero
/// initial state, coefficients normalized by `a[0]`.
pub fn lfilter(b: &[f64], a: &[f64], x: &mut [f64]) {
    let n = b.len().max(a.len());
    let a0 = a[0];
    let coef = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(0.0) / a0;
    let (b, a): (Vec<f64>, Vec<f64>) = (
        (0..n).map(|i| coef(b, i)).collect(),
        (0..n).map(|i| coef(a, i)).collect(),
    );
    let mut z = vec![0.0; n.saturating_sub(1)];
    for xn in x.iter_mut() {
        let xv = *xn;
        if z.is_empty() {
            *xn = b[0] * xv;
            continue;
        }
        let y = z[0] + b[0] * xv;
        for k in 0..z.len() - 1 {
            z[k] = z[k + 1] + xv * b[k + 1] - y * a[k + 1];
        }
        let last = z.len() - 1;
        z[last] = xv * b[n - 1] - y * a[n - 1];
        *xn = y;
    }
}

fn length(tau: f64, sr: f64) -> usize {
    let n = (10.0 * tau * sr) as i64 + (0.02 * sr) as i64;
    ((0.04 * sr) as i64).max(((MAX_SECONDS * sr) as i64).min(n)) as usize
}

fn env(t: &[f64], tau: f64) -> Vec<f64> {
    let m = tau.max(1e-4);
    t.iter().map(|&x| (-x / m).exp()).collect()
}

fn normals(rng: &mut NumpyRng, n: usize) -> Vec<f64> {
    (0..n).map(|_| rng.standard_normal()).collect()
}

/// White noise through a first-order high-pass at `cutoff` Hz.
fn noise(rng: &mut NumpyRng, n: usize, sr: f64, cutoff: f64) -> Vec<f64> {
    let mut x = normals(rng, n);
    let a = 1.0 / (1.0 + TWO_PI * cutoff.max(100.0).min(0.45 * sr) / sr);
    lfilter(&[a, -a], &[1.0, -a], &mut x);
    x
}

/// White noise through a wide band-pass centred on `centre` Hz (1.5 octaves).
fn band_noise(rng: &mut NumpyRng, n: usize, sr: f64, centre: f64) -> Vec<f64> {
    let octaves = 1.5;
    let low = (centre * 2.0_f64.powf(-octaves)).max(30.0);
    let high = (centre * 2.0_f64.powf(octaves)).min(0.45 * sr);
    if high <= low {
        return vec![0.0; n];
    }
    let (b, a) = butter1_bandpass(low / (sr / 2.0), high / (sr / 2.0));
    let mut x = normals(rng, n);
    lfilter(&b, &a, &mut x);
    x
}

fn tone(freq: &[f64], sr: f64) -> Vec<f64> {
    let mut acc = 0.0;
    freq.iter()
        .map(|&f| {
            acc += f;
            (TWO_PI * acc / sr).sin()
        })
        .collect()
}

fn mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x * y).collect()
}

fn tau_of(kind: usize, decay: f64) -> f64 {
    match kind {
        0 => 0.08 + 0.30 * decay,
        1 => 0.05 + 0.18 * decay,
        2 => 0.04 + 0.12 * decay,
        3 => 0.12 + 0.30 * decay,
        4 => 0.02 + 0.05 * decay,
        _ => 0.12 + 0.20 * decay,
    }
}

/// Mono `f32` one-shot for instrument `kind` (index into `INSTRUMENTS`),
/// normalized to a 0.9 peak and deterministic. `tune`/`decay` are clamped
/// through the drum registry (non-finite -> default). Panics on a bad `kind`.
pub fn render_one_shot(kind: usize, sample_rate: f64, tune: f64, decay: f64) -> Vec<f32> {
    let name = INSTRUMENTS[kind];
    let spec = |field: &str| find(&DRUM_PARAMS, &format!("{name}_{field}")).unwrap();
    let (tune, decay) = (spec("tune").clamp(tune), spec("decay").clamp(decay));
    let sr = sample_rate;
    let mut rng = NumpyRng::new(SEED[kind]);
    let tau = tau_of(kind, decay);
    let n = length(tau, sr);
    let t: Vec<f64> = (0..n).map(|i| i as f64 / sr).collect();

    let mut y: Vec<f64> = match kind {
        0 => {
            let base = 55.0 * 2.0_f64.powf(tune * 0.5);
            let freq: Vec<f64> = t
                .iter()
                .map(|&x| base * (1.0 + 1.6 * (-x / 0.030).exp()))
                .collect();
            let body = mul(&tone(&freq, sr), &env(&t, 0.08 + 0.30 * decay));
            let click = mul(&noise(&mut rng, n, sr, 2000.0), &env(&t, 0.002));
            body.iter().zip(&click).map(|(b, c)| b + c * 0.3).collect()
        }
        1 => {
            let shift = 2.0_f64.powf(tune * 0.3);
            let t1 = tone(&vec![180.0 * shift; n], sr);
            let t2 = tone(&vec![330.0 * shift; n], sr);
            let tn: Vec<f64> = t1.iter().zip(&t2).map(|(a, b)| a + 0.7 * b).collect();
            let body = mul(&tn, &env(&t, 0.05 + 0.10 * decay));
            let nz = noise(&mut rng, n, sr, 1200.0 * 2.0_f64.powf(tune * 1.5));
            let nz = mul(&nz, &env(&t, 0.05 + 0.18 * decay));
            body.iter().zip(&nz).map(|(b, z)| 0.8 * b + z).collect()
        }
        2 => {
            let spread = 2.0_f64.powf(tune * 0.25);
            let mut bursts = vec![0.0; n];
            for i in 0..4 {
                let centre = i as f64 * 0.010 / spread;
                for (b, &x) in bursts.iter_mut().zip(&t) {
                    let u = (x - centre) / 0.0035;
                    *b += (-(u * u)).exp();
                }
            }
            let tail: Vec<f64> = env(&t, 0.04 + 0.12 * decay)
                .iter()
                .map(|e| 0.6 * e)
                .collect();
            let band = band_noise(&mut rng, n, sr, 1000.0 * 2.0_f64.powf(tune * 1.5));
            band.iter()
                .zip(bursts.iter().zip(&tail))
                .map(|(z, (b, l))| z * (b + l))
                .collect()
        }
        3 => {
            let base = 100.0 * 2.0_f64.powf(tune * 0.5);
            let freq: Vec<f64> = t
                .iter()
                .map(|&x| base * (1.0 + 0.6 * (-x / 0.060).exp()))
                .collect();
            mul(&tone(&freq, sr), &env(&t, 0.12 + 0.30 * decay))
        }
        _ => {
            let band = band_noise(&mut rng, n, sr, 6500.0 * 2.0_f64.powf(tune * 1.5));
            mul(&band, &env(&t, tau))
        }
    };

    let fade = ((TAIL_FADE_S * sr) as usize).min(n);
    if fade > 1 {
        // np.linspace(1, 0, fade): i*step + 1, last sample exactly 0.
        let step = -1.0 / (fade - 1) as f64;
        for (j, v) in y[n - fade..].iter_mut().enumerate() {
            *v *= if j == fade - 1 {
                0.0
            } else {
                j as f64 * step + 1.0
            };
        }
    }
    let peak = y.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        let g = PEAK / peak;
        y.iter_mut().for_each(|v| *v *= g);
    }
    y.into_iter().map(|v| v as f32).collect()
}
