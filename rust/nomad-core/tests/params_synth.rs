mod common;

use common::{close, load};
use nomad_core::params::{Curve, SYNTH_PARAMS, find};

fn curve_name(c: Curve) -> &'static str {
    match c {
        Curve::Lin => "lin",
        Curve::Exp => "exp",
    }
}

#[test]
fn registry_matches_python() {
    let r = load();
    assert_eq!(r.synth.len(), 63);
    assert_eq!(SYNTH_PARAMS.len(), r.synth.len());
    for (s, p) in SYNTH_PARAMS.iter().zip(&r.synth) {
        assert_eq!(s.name, p.name);
        assert_eq!(s.unit, p.unit, "{}", p.name);
        assert_eq!(curve_name(s.curve), p.curve, "{}", p.name);
        assert_eq!(s.default, p.default, "{}", p.name);
        assert_eq!(s.lo, p.lo, "{}", p.name);
        assert_eq!(s.hi, p.hi, "{}", p.name);
        assert_eq!(s.smooth_ms, p.smooth_ms, "{}", p.name);
    }
}

#[test]
fn clamp_matches_python() {
    for (s, p) in SYNTH_PARAMS.iter().zip(&load().synth) {
        for (&x, &out) in p.clamp_in.iter().zip(&p.clamp_out) {
            assert!(close(s.clamp(x), out), "{} clamp({x})", p.name);
        }
    }
}

#[test]
fn clamp_non_finite_returns_default() {
    for s in &SYNTH_PARAMS {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(s.clamp(x), s.default, "{} clamp({x})", s.name);
        }
    }
}

#[test]
fn to_norm_matches_python() {
    for (s, p) in SYNTH_PARAMS.iter().zip(&load().synth) {
        for (&x, &out) in p.clamp_in.iter().zip(&p.to_norm_out) {
            assert!(close(s.to_norm(x), out), "{} to_norm({x})", p.name);
        }
    }
}

#[test]
fn from_norm_matches_python() {
    for (s, p) in SYNTH_PARAMS.iter().zip(&load().synth) {
        for (&n, &out) in p.from_norm_in.iter().zip(&p.from_norm_out) {
            assert!(close(s.from_norm(n), out), "{} from_norm({n})", p.name);
        }
    }
}

#[test]
fn specs_are_valid() {
    for s in &SYNTH_PARAMS {
        assert!(s.lo < s.hi, "{}", s.name);
        assert!(s.lo <= s.default && s.default <= s.hi, "{}", s.name);
        if s.curve == Curve::Exp {
            assert!(s.lo > 0.0, "{}", s.name);
        }
    }
}

#[test]
fn find_known_and_unknown() {
    assert_eq!(find(&SYNTH_PARAMS, "glide").map(|s| s.hi), Some(1000.0));
    assert!(find(&SYNTH_PARAMS, "nope").is_none());
}

#[test]
fn from_norm_nan_propagates_like_python() {
    // Python: min(max(nan, 0.0), 1.0) is nan, so from_norm returns nan.
    for s in &SYNTH_PARAMS {
        assert!(s.from_norm(f64::NAN).is_nan(), "{}", s.name);
    }
}
