//! Declarative parameter registry, ported from `nomad/engine/params.py`.
//!
//! Same names, ranges, curves, units and order as the Python registry.
//! The tables are `static` (no heap), so they are safe to read from the audio thread.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Curve {
    Lin,
    Exp,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub default: f64,
    pub lo: f64,
    pub hi: f64,
    pub curve: Curve,
    pub unit: &'static str,
    pub smooth_ms: f64,
}

impl ParamSpec {
    /// Limit `v` to `[lo, hi]`; non-finite input returns `default` (as in Python).
    pub fn clamp(&self, v: f64) -> f64 {
        if !v.is_finite() {
            return self.default;
        }
        v.clamp(self.lo, self.hi)
    }

    /// Map a raw value (clamped first) onto its 0..1 knob position.
    pub fn to_norm(&self, v: f64) -> f64 {
        let v = self.clamp(v);
        match self.curve {
            Curve::Exp => (v / self.lo).ln() / (self.hi / self.lo).ln(),
            Curve::Lin => (v - self.lo) / (self.hi - self.lo),
        }
    }

    /// Map a 0..1 knob position (clamped first) back to a raw value.
    pub fn from_norm(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0); // NaN stays NaN, as in Python
        match self.curve {
            Curve::Exp => self.lo * (self.hi / self.lo).powf(n),
            Curve::Lin => self.lo + n * (self.hi - self.lo),
        }
    }
}

/// Look up a spec by name. Unlike Python (`KeyError`), an unknown name returns `None`.
pub fn find<'a>(list: &'a [ParamSpec], name: &str) -> Option<&'a ParamSpec> {
    list.iter().find(|s| s.name == name)
}

const fn spec(
    name: &'static str,
    default: f64,
    lo: f64,
    hi: f64,
    curve: Curve,
    unit: &'static str,
) -> ParamSpec {
    ParamSpec {
        name,
        default,
        lo,
        hi,
        curve,
        unit,
        smooth_ms: 15.0,
    }
}

/// Synth registry in the canonical UI order (includes `master_volume`).
pub static SYNTH_PARAMS: [ParamSpec; 63] = [
    spec("polyphony", 8.0, 1.0, 8.0, Curve::Lin, "voices"),
    spec("glide", 0.0, 0.0, 1000.0, Curve::Lin, "ms"),
    spec("master_volume", 0.7, 0.0, 1.0, Curve::Lin, ""),
    spec("osc1_wave", 2.0, 0.0, 3.0, Curve::Lin, ""),
    spec("osc1_mode", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("osc1_coarse", 0.0, -24.0, 24.0, Curve::Lin, "st"),
    spec("osc1_hz", 220.0, 20.0, 8000.0, Curve::Exp, "Hz"),
    spec("osc1_fine", 0.0, -50.0, 50.0, Curve::Lin, "ct"),
    spec("osc1_pw", 0.5, 0.05, 0.95, Curve::Lin, ""),
    spec("osc1_level", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("osc1_sync", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("osc2_wave", 2.0, 0.0, 3.0, Curve::Lin, ""),
    spec("osc2_mode", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("osc2_coarse", 0.0, -24.0, 24.0, Curve::Lin, "st"),
    spec("osc2_hz", 220.0, 20.0, 8000.0, Curve::Exp, "Hz"),
    spec("osc2_fine", 0.0, -50.0, 50.0, Curve::Lin, "ct"),
    spec("osc2_pw", 0.5, 0.05, 0.95, Curve::Lin, ""),
    spec("osc2_level", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("osc2_sync", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("filt_cutoff", 1200.0, 20.0, 18000.0, Curve::Exp, "Hz"),
    spec("filt_reso", 0.3, 0.0, 1.0, Curve::Lin, ""),
    spec("filt_env_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("amp_attack", 5.0, 1.0, 5000.0, Curve::Exp, "ms"),
    spec("amp_decay", 300.0, 1.0, 5000.0, Curve::Exp, "ms"),
    spec("amp_sustain", 0.8, 0.0, 1.0, Curve::Lin, ""),
    spec("amp_release", 400.0, 5.0, 8000.0, Curve::Exp, "ms"),
    spec("filt_attack", 5.0, 1.0, 5000.0, Curve::Exp, "ms"),
    spec("filt_decay", 250.0, 1.0, 5000.0, Curve::Exp, "ms"),
    spec("filt_sustain", 0.3, 0.0, 1.0, Curve::Lin, ""),
    spec("filt_release", 400.0, 5.0, 8000.0, Curve::Exp, "ms"),
    spec("lfo1_rate", 1.0, 0.02, 40.0, Curve::Exp, "Hz"),
    spec("lfo1_wave", 0.0, 0.0, 4.0, Curve::Lin, ""),
    spec("lfo1_depth", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("lfo1_phase", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("lfo2_rate", 1.0, 0.02, 40.0, Curve::Exp, "Hz"),
    spec("lfo2_wave", 0.0, 0.0, 4.0, Curve::Lin, ""),
    spec("lfo2_depth", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("lfo2_phase", 0.0, 0.0, 1.0, Curve::Lin, ""),
    spec("mod0_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod0_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod0_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod1_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod1_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod1_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod2_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod2_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod2_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod3_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod3_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod3_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod4_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod4_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod4_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod5_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod5_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod5_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod6_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod6_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod6_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("mod7_src", 0.0, 0.0, 5.0, Curve::Lin, ""),
    spec("mod7_dst", 0.0, 0.0, 7.0, Curve::Lin, ""),
    spec("mod7_amt", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("bend_range", 2.0, 1.0, 12.0, Curve::Lin, "st"),
];

/// Machine Drum instruments, in the order of `nomad/engine/drums.py`.
pub const INSTRUMENTS: [&str; 6] = ["kick", "snare", "clap", "tom", "chat", "ohat"];

/// Drum registry (`DRUM_REGISTRY`): tune/decay/level per instrument, then bpm and swing.
pub static DRUM_PARAMS: [ParamSpec; 20] = [
    spec("kick_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("kick_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("kick_level", 0.6, 0.0, 1.0, Curve::Lin, ""),
    spec("snare_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("snare_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("snare_level", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("clap_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("clap_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("clap_level", 0.45, 0.0, 1.0, Curve::Lin, ""),
    spec("tom_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("tom_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("tom_level", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("chat_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("chat_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("chat_level", 0.35, 0.0, 1.0, Curve::Lin, ""),
    spec("ohat_tune", 0.0, -1.0, 1.0, Curve::Lin, ""),
    spec("ohat_decay", 0.5, 0.0, 1.0, Curve::Lin, ""),
    spec("ohat_level", 0.3, 0.0, 1.0, Curve::Lin, ""),
    spec("bpm", 110.0, 60.0, 180.0, Curve::Lin, "BPM"),
    spec("swing", 0.0, 0.0, 0.6, Curve::Lin, ""),
];

/// Index of each synth parameter in `SYNTH_PARAMS` (and in `ParamValues`).
pub mod pid {
    pub const POLYPHONY: usize = 0;
    pub const GLIDE: usize = 1;
    pub const MASTER_VOLUME: usize = 2;
    pub const OSC1_WAVE: usize = 3;
    pub const OSC1_MODE: usize = 4;
    pub const OSC1_COARSE: usize = 5;
    pub const OSC1_HZ: usize = 6;
    pub const OSC1_FINE: usize = 7;
    pub const OSC1_PW: usize = 8;
    pub const OSC1_LEVEL: usize = 9;
    pub const OSC1_SYNC: usize = 10;
    pub const OSC2_WAVE: usize = 11;
    pub const OSC2_MODE: usize = 12;
    pub const OSC2_COARSE: usize = 13;
    pub const OSC2_HZ: usize = 14;
    pub const OSC2_FINE: usize = 15;
    pub const OSC2_PW: usize = 16;
    pub const OSC2_LEVEL: usize = 17;
    pub const OSC2_SYNC: usize = 18;
    pub const FILT_CUTOFF: usize = 19;
    pub const FILT_RESO: usize = 20;
    pub const FILT_ENV_AMT: usize = 21;
    pub const AMP_ATTACK: usize = 22;
    pub const AMP_DECAY: usize = 23;
    pub const AMP_SUSTAIN: usize = 24;
    pub const AMP_RELEASE: usize = 25;
    pub const FILT_ATTACK: usize = 26;
    pub const FILT_DECAY: usize = 27;
    pub const FILT_SUSTAIN: usize = 28;
    pub const FILT_RELEASE: usize = 29;
    pub const LFO1_RATE: usize = 30;
    pub const LFO1_WAVE: usize = 31;
    pub const LFO1_DEPTH: usize = 32;
    pub const LFO1_PHASE: usize = 33;
    pub const LFO2_RATE: usize = 34;
    pub const LFO2_WAVE: usize = 35;
    pub const LFO2_DEPTH: usize = 36;
    pub const LFO2_PHASE: usize = 37;
    pub const MOD0_SRC: usize = 38;
    pub const MOD0_DST: usize = 39;
    pub const MOD0_AMT: usize = 40;
    pub const MOD1_SRC: usize = 41;
    pub const MOD1_DST: usize = 42;
    pub const MOD1_AMT: usize = 43;
    pub const MOD2_SRC: usize = 44;
    pub const MOD2_DST: usize = 45;
    pub const MOD2_AMT: usize = 46;
    pub const MOD3_SRC: usize = 47;
    pub const MOD3_DST: usize = 48;
    pub const MOD3_AMT: usize = 49;
    pub const MOD4_SRC: usize = 50;
    pub const MOD4_DST: usize = 51;
    pub const MOD4_AMT: usize = 52;
    pub const MOD5_SRC: usize = 53;
    pub const MOD5_DST: usize = 54;
    pub const MOD5_AMT: usize = 55;
    pub const MOD6_SRC: usize = 56;
    pub const MOD6_DST: usize = 57;
    pub const MOD6_AMT: usize = 58;
    pub const MOD7_SRC: usize = 59;
    pub const MOD7_DST: usize = 60;
    pub const MOD7_AMT: usize = 61;
    pub const BEND_RANGE: usize = 62;
    /// Number of synth parameters.
    pub const COUNT: usize = 63;
}

/// Synth parameter values, indexed by `pid`.
pub type ParamValues = [f64; pid::COUNT];

/// The registry defaults, in `pid` order.
pub fn default_values() -> ParamValues {
    std::array::from_fn(|i| SYNTH_PARAMS[i].default)
}
