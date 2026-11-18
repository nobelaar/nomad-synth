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
        v.max(self.lo).min(self.hi)
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
        let n = n.max(0.0).min(1.0);
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
