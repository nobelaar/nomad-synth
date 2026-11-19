//! `KnobModel` of `nomad/gui/knob.py`: value <-> norm through the spec's curve,
//! drag/wheel gestures and the display format, with Python's formatting rules.

use nomad_core::params::{Curve, ParamSpec};

/// Pixels of vertical drag for the full 0..1 travel.
pub const DRAG_PX: f64 = 200.0;
/// Shift divides the drag/wheel step by this.
pub const FINE: f64 = 10.0;
/// Norm per wheel notch (50 notches = full travel).
pub const WHEEL_STEP: f64 = 0.02;

/// A non-finite norm or value was rejected (Python raises `ValueError`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotFinite;

#[derive(Clone, Debug)]
pub struct KnobModel {
    pub spec: &'static ParamSpec,
    norm: f64,
}

impl KnobModel {
    /// A non-finite `value` leaves the knob at 0 (Python would raise).
    pub fn new(spec: &'static ParamSpec, value: f64) -> KnobModel {
        let mut m = KnobModel { spec, norm: 0.0 };
        let _ = m.set_value(value);
        m
    }

    pub fn norm(&self) -> f64 {
        self.norm
    }

    pub fn set_norm(&mut self, n: f64) -> Result<(), NotFinite> {
        if !n.is_finite() {
            return Err(NotFinite);
        }
        self.norm = n.clamp(0.0, 1.0);
        Ok(())
    }

    pub fn value(&self) -> f64 {
        let s = self.spec;
        match s.curve {
            Curve::Exp => s.lo * (s.hi / s.lo).powf(self.norm),
            Curve::Lin => s.lo + (s.hi - s.lo) * self.norm,
        }
    }

    pub fn set_value(&mut self, v: f64) -> Result<(), NotFinite> {
        if !v.is_finite() {
            return Err(NotFinite);
        }
        let s = self.spec;
        let v = v.clamp(s.lo, s.hi);
        let n = match s.curve {
            Curve::Exp => (v / s.lo).ln() / (s.hi / s.lo).ln(),
            Curve::Lin => (v - s.lo) / (s.hi - s.lo),
        };
        self.set_norm(n)
    }

    /// Incremental: positive pixels (drag up) increase the value.
    pub fn set_from_drag(&mut self, pixels: f64, fine: bool) {
        let _ = self.set_norm(self.norm + pixels / DRAG_PX / if fine { FINE } else { 1.0 });
    }

    /// `delta` notches (+ up): 0.02 of travel, 0.002 with fine.
    pub fn set_from_wheel(&mut self, delta: i32, fine: bool) {
        let _ = self
            .set_norm(self.norm + f64::from(delta) * WHEEL_STEP / if fine { FINE } else { 1.0 });
    }

    pub fn reset(&mut self) {
        let _ = self.set_value(self.spec.default);
    }

    pub fn format_value(&self) -> String {
        let (s, v, u) = (self.spec, self.value(), self.spec.unit);
        match u {
            "ms" if v >= 1000.0 => format!("{} s", fixed(v / 1000.0, 2)),
            "ms" if v >= 100.0 => format!("{} ms", fixed(v, 0)),
            "ms" => format!("{} ms", fixed(v, 1)),
            "Hz" if v >= 10000.0 => format!("{} kHz", fixed(v / 1000.0, 1)),
            "Hz" if v >= 1000.0 => format!("{} kHz", fixed(v / 1000.0, 2)),
            "Hz" if v >= 100.0 => format!("{} Hz", fixed(v, 0)),
            "Hz" if v >= 10.0 => format!("{} Hz", fixed(v, 1)),
            "Hz" => format!("{} Hz", fixed(v, 2)),
            "st" | "ct" => format!("{} {u}", signed_fixed0(v)),
            "voices" => format!("{} voices", fixed(v, 0)),
            "" => {
                let pct = (v * 100.0).round_ties_even() as i64;
                if s.lo < 0.0 {
                    format!("{pct:+}%")
                } else {
                    format!("{pct}%")
                }
            }
            _ => format!("{} {u}", general(v)),
        }
    }
}

/// Python `f"{v:.{p}f}"` (exact decimal rounding, ties to even, like Rust's `{:.p}`).
fn fixed(v: f64, p: usize) -> String {
    format!("{v:.p$}")
}

/// Python `f"{v:+.0f}"`: always signed, `-0` for small negatives.
fn signed_fixed0(v: f64) -> String {
    let s = format!("{v:.0}");
    if s.starts_with('-') {
        s
    } else {
        format!("+{s}")
    }
}

/// Python `f"{v:g}"`: 6 significant digits, trailing zeros stripped, exponent
/// form when the exponent is < -4 or >= 6.
pub fn general(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".into()
        } else if v > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let sci = format!("{v:.5e}");
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if (-4..6).contains(&exp) {
        strip(&format!("{v:.p$}", p = (5 - exp) as usize))
    } else {
        format!(
            "{}e{}{:02}",
            strip(mant),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}
