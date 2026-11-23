//! One synth voice: two oscillators, ladder filter, two ADSRs, glide and a
//! click-free steal. Ported from `nomad/engine/voice.py` (the specification).
//!
//! `render` is called once per control block; oscillators and envelopes run
//! over the whole block and only the filter is split into grains (16 samples
//! when k > 3.2, else 64) with the log2 cutoff smoothed per grain.

use crate::envelope::Envelope;
use crate::filter::{K_MAX, Ladder4Pole};
use crate::mods::Mods;
use crate::oscillator::{MAX_BLOCK, MAX_INC, Oscillator, Wave};
use crate::params::{ParamValues, pid};

pub const STEAL_FADE_SEC: f64 = 0.005;
const CUTOFF_TAU: f64 = 0.003;

/// Python's `_f`: non-finite -> `default`.
fn f(x: f64, default: f64) -> f64 {
    if x.is_finite() { x } else { default }
}

fn wave(x: f64) -> Wave {
    // Python raises on an unknown wave; the registry clamps 0..3, so fall back to saw.
    Wave::from_index(f(x, 2.0) as i64).unwrap_or(Wave::Saw)
}

pub struct Voice {
    pub note: i32,
    pub age: u64,
    pub velocity: f64,
    pub env2_level: f64,
    /// What the last block played, for the UI (Hz of osc1/osc2, filter cutoff in
    /// Hz, amp envelope x gain); not part of the sound.
    pub last_hz: [f64; 2],
    pub last_cutoff: f64,
    pub last_amp: f64,
    sr: f64,
    osc1: Oscillator,
    osc2: Oscillator,
    filter: Ladder4Pole,
    amp_env: Envelope,
    filt_env: Envelope,
    /// Key the oscillators follow (the old note during a steal).
    pitch_note: i32,
    /// Pitch actually sounding, in semitones; glides towards `pitch_note`.
    cur: f64,
    /// Smoothed log2 cutoff; `None` = initialise on the next grain.
    lc: Option<f64>,
    fade_left: i64,
    pending: Option<(i32, f64)>,
    aenv: Vec<f64>,
    fenv: Vec<f64>,
    mix: Vec<f64>,
    tmp: Vec<f64>,
    master: Vec<f64>,
}

impl Voice {
    pub fn new(sample_rate: f64) -> Voice {
        Voice {
            note: -1,
            age: 0,
            velocity: 0.0,
            env2_level: 0.0,
            last_hz: [0.0; 2],
            last_cutoff: 0.0,
            last_amp: 0.0,
            sr: sample_rate,
            osc1: Oscillator::new(sample_rate),
            osc2: Oscillator::new(sample_rate),
            filter: Ladder4Pole::new(sample_rate),
            amp_env: Envelope::new(sample_rate),
            filt_env: Envelope::new(sample_rate),
            pitch_note: 60,
            cur: 60.0,
            lc: None,
            fade_left: 0,
            pending: None,
            aenv: vec![0.0; MAX_BLOCK],
            fenv: vec![0.0; MAX_BLOCK],
            mix: vec![0.0; MAX_BLOCK],
            tmp: vec![0.0; MAX_BLOCK],
            master: vec![0.0; MAX_BLOCK],
        }
    }

    pub fn is_active(&self) -> bool {
        self.pending.is_some() || self.fade_left > 0 || !self.amp_env.is_idle()
    }

    fn fade_total(&self) -> i64 {
        ((STEAL_FADE_SEC * self.sr) as i64).max(1)
    }

    fn trigger(&mut self, note: i32, velocity: f64, start: Option<f64>) {
        if self.amp_env.is_idle() {
            // Fresh start: deterministic phase and clean filter. A steal keeps `cur`.
            self.osc1.phase = 0.0;
            self.osc2.phase = 0.0;
            self.filter.reset();
            self.lc = None;
            self.cur = start.unwrap_or(note as f64);
        }
        self.note = note;
        self.pitch_note = note;
        self.velocity = f(velocity, 0.0).clamp(0.0, 1.0);
        self.amp_env.note_on();
        self.filt_env.note_on();
    }

    pub fn note_on(&mut self, note: i32, velocity: f64, start: Option<f64>) {
        self.pending = None;
        self.fade_left = 0;
        self.trigger(note, velocity, start);
    }

    /// Legato: glide to `note` without touching the envelopes.
    pub fn set_target(&mut self, note: i32) {
        self.note = note;
        self.pitch_note = note;
    }

    pub fn note_off(&mut self) {
        if self.pending.is_some() {
            // Cancel the retrigger; the fade ends in silence.
            self.pending = None;
            self.fade_left = self.fade_left.max(1);
        }
        self.amp_env.note_off();
        self.filt_env.note_off();
    }

    pub fn begin_steal(&mut self, note: i32, velocity: f64) {
        if !self.is_active() {
            return self.note_on(note, velocity, None);
        }
        self.note = note; // so the manager's note_off lookup finds the new key
        self.pending = Some((note, velocity));
        // A second steal during a running fade only retargets the pending note.
        if self.fade_left == 0 {
            self.fade_left = self.fade_total();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn freq(
        &self,
        p: &ParamValues,
        m: &Mods,
        mode: usize,
        hz: usize,
        coarse: usize,
        fine: usize,
        mod_semis: f64,
    ) -> f64 {
        let mut semis = f(mod_semis, 0.0) + f(m.pitch, 0.0);
        let base;
        if f(p[mode], 0.0) >= 0.5 {
            // Free: absolute Hz, ignores key and bend.
            base = f(p[hz], 220.0);
            semis += f(p[fine], 0.0) / 100.0;
        } else {
            semis += self.cur - 69.0 + f(p[coarse], 0.0) + f(p[fine], 0.0) / 100.0 + f(m.bend, 0.0);
            base = 440.0;
        }
        base * 2.0_f64.powf(semis.clamp(-200.0, 200.0) / 12.0)
    }

    /// Add one block of this voice into `out`.
    pub fn render(&mut self, out: &mut [f64], p: &ParamValues, mods: &Mods) {
        let n = out.len();
        if n == 0 || !self.is_active() {
            return;
        }
        if n > self.mix.len() {
            // shortcut: blocks above MAX_BLOCK grow the scratch once (allocates).
            for b in [
                &mut self.aenv,
                &mut self.fenv,
                &mut self.mix,
                &mut self.tmp,
                &mut self.master,
            ] {
                b.resize(n, 0.0);
            }
        }
        let sr = self.sr;
        self.amp_env.set_params(
            p[pid::AMP_ATTACK] / 1e3,
            p[pid::AMP_DECAY] / 1e3,
            p[pid::AMP_SUSTAIN],
            p[pid::AMP_RELEASE] / 1e3,
        );
        self.filt_env.set_params(
            p[pid::FILT_ATTACK] / 1e3,
            p[pid::FILT_DECAY] / 1e3,
            p[pid::FILT_SUSTAIN],
            p[pid::FILT_RELEASE] / 1e3,
        );
        self.amp_env.process(&mut self.aenv[..n]);
        self.filt_env.process(&mut self.fenv[..n]);
        self.env2_level = self.fenv[n - 1];

        // Portamento (shortcut: one frequency per block, so glides under ~20 ms sound stepped).
        let glide = f(p[pid::GLIDE], 0.0);
        let d = self.pitch_note as f64 - self.cur;
        if glide <= 0.0 || d.abs() < 1e-3 {
            self.cur = self.pitch_note as f64;
        } else {
            // RC curve: glide ms ~ 3 time constants.
            self.cur += d * (1.0 - (-(n as f64) / (sr * glide / 3000.0)).exp());
        }

        // Oscillators: whole block each.
        let f1 = self.freq(
            p,
            mods,
            pid::OSC1_MODE,
            pid::OSC1_HZ,
            pid::OSC1_COARSE,
            pid::OSC1_FINE,
            mods.pitch1,
        );
        let f2 = self.freq(
            p,
            mods,
            pid::OSC2_MODE,
            pid::OSC2_HZ,
            pid::OSC2_COARSE,
            pid::OSC2_FINE,
            mods.pitch2,
        );
        self.osc1.wave = wave(p[pid::OSC1_WAVE]);
        self.osc2.wave = wave(p[pid::OSC2_WAVE]);
        let sync = f(p[pid::OSC2_SYNC], 0.0) >= 0.5;
        let mix = &mut self.mix[..n];
        let tmp = &mut self.tmp[..n];
        let master = &mut self.master[..n];
        if sync {
            // osc1's phase for these samples, same ramp the oscillator will use.
            let inc = f1.abs().min(MAX_INC * sr) / sr;
            let ph = self.osc1.phase;
            for (i, m) in master.iter_mut().enumerate() {
                *m = (i as f64 * inc + ph).rem_euclid(1.0);
            }
        }
        tmp.fill(0.0);
        let pw1 = f(p[pid::OSC1_PW], 0.5) + 0.45 * f(mods.pw1, 0.0);
        self.osc1.render_into(tmp, f1, pw1, None);
        let level1 = f(p[pid::OSC1_LEVEL], 0.0);
        for (m, t) in mix.iter_mut().zip(tmp.iter()) {
            *m = t * level1;
        }
        tmp.fill(0.0);
        let pw2 = f(p[pid::OSC2_PW], 0.5) + 0.45 * f(mods.pw2, 0.0);
        self.osc2
            .render_into(tmp, f2, pw2, sync.then_some(&*master));
        let level2 = f(p[pid::OSC2_LEVEL], 0.0);
        for (m, t) in mix.iter_mut().zip(tmp.iter()) {
            *m += t * level2;
        }

        // Filter: grains, cutoff recomputed and smoothed per grain.
        let reso = f(p[pid::FILT_RESO], 0.0).clamp(0.0, 1.0);
        let grain = if reso * K_MAX > 3.2 { 16 } else { 64 };
        let base = f(p[pid::FILT_CUTOFF], 1200.0).clamp(20.0, 18000.0).log2();
        let amt = 5.0 * f(p[pid::FILT_ENV_AMT], 0.0);
        let mc = 2.0 * f(mods.cutoff, 0.0);
        let coef = 1.0 - (-(grain as f64) / (CUTOFF_TAU * sr)).exp();
        let (log_lo, log_hi) = (20.0_f64.log2(), 18000.0_f64.log2());
        let mut lc = self.lc;
        for s in (0..n).step_by(grain) {
            let e = n.min(s + grain);
            let target = (base + amt * self.fenv[s] + mc).clamp(log_lo, log_hi);
            let l = match lc {
                None => target,
                Some(l) => l + coef * (target - l),
            };
            lc = Some(l);
            self.filter.set(2.0_f64.powf(l), reso);
            self.filter.process(&mut mix[s..e]);
        }
        self.lc = lc;
        self.last_hz = [f1, f2];
        self.last_cutoff = lc.map_or(0.0, f64::exp2);

        // Amp: envelope, velocity, mod, steal fade.
        let gain = self.velocity * (1.0 + f(mods.amp, 0.0)).max(0.0);
        self.last_amp = self.aenv[n - 1] * gain;
        for (m, a) in mix.iter_mut().zip(&self.aenv[..n]) {
            *m *= a;
            *m *= gain;
        }
        if self.fade_left > 0 {
            let inv = 1.0 / ((STEAL_FADE_SEC * sr) as i64).max(1) as f64;
            let fl = self.fade_left as f64;
            for (i, m) in mix.iter_mut().enumerate() {
                *m *= ((fl - i as f64) * inv).clamp(0.0, 1.0);
            }
            self.fade_left = (self.fade_left - n as i64).max(0);
            if self.fade_left == 0 {
                self.amp_env.value = 0.0; // output gain is 0 here: no jump
                match self.pending.take() {
                    Some((note, vel)) => self.trigger(note, vel, None),
                    None => self.amp_env.note_off(),
                }
            }
        }
        for (o, m) in out.iter_mut().zip(self.mix[..n].iter()) {
            // nan_to_num: NaN and infinities become 0.
            *o += if m.is_finite() { *m } else { 0.0 };
        }
    }
}
