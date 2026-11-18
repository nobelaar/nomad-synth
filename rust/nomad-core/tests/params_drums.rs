mod common;

use common::{close, load};
use nomad_core::params::{Curve, DRUM_PARAMS, INSTRUMENTS, find};

#[test]
fn registry_matches_python() {
    let r = load();
    assert_eq!(r.drums.len(), 20);
    assert_eq!(DRUM_PARAMS.len(), r.drums.len());
    for (s, p) in DRUM_PARAMS.iter().zip(&r.drums) {
        let curve = if s.curve == Curve::Exp { "exp" } else { "lin" };
        assert_eq!(s.name, p.name);
        assert_eq!(s.unit, p.unit, "{}", p.name);
        assert_eq!(curve, p.curve, "{}", p.name);
        assert_eq!(s.default, p.default, "{}", p.name);
        assert_eq!(s.lo, p.lo, "{}", p.name);
        assert_eq!(s.hi, p.hi, "{}", p.name);
        assert_eq!(s.smooth_ms, p.smooth_ms, "{}", p.name);
    }
}

#[test]
fn clamp_matches_python() {
    for (s, p) in DRUM_PARAMS.iter().zip(&load().drums) {
        for (&x, &out) in p.clamp_in.iter().zip(&p.clamp_out) {
            assert!(close(s.clamp(x), out), "{} clamp({x})", p.name);
        }
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(s.clamp(x), s.default, "{} clamp({x})", s.name);
        }
    }
}

#[test]
fn instrument_order() {
    assert_eq!(
        INSTRUMENTS,
        ["kick", "snare", "clap", "tom", "chat", "ohat"]
    );
    for i in INSTRUMENTS {
        for p in ["tune", "decay", "level"] {
            assert!(find(&DRUM_PARAMS, &format!("{i}_{p}")).is_some(), "{i}_{p}");
        }
    }
}

#[test]
fn bpm_and_swing_ranges() {
    let bpm = find(&DRUM_PARAMS, "bpm").unwrap();
    assert_eq!((bpm.lo, bpm.hi, bpm.default), (60.0, 180.0, 110.0));
    let swing = find(&DRUM_PARAMS, "swing").unwrap();
    assert_eq!((swing.lo, swing.hi, swing.default), (0.0, 0.6, 0.0));
}
