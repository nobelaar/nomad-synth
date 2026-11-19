mod common;

use common::{Blob, assert_close, num, nums};
use nomad_core::lfo::Lfo;

const SR: f64 = 48000.0;

fn run(m: &serde_json::Value) -> (Vec<f64>, Lfo) {
    let mut lfo = Lfo::new(SR, m["seed"].as_u64().unwrap());
    lfo.set(
        num(&m["rate"]),
        m["wave"].as_i64().unwrap(),
        num(&m["depth"]),
        num(&m["offset"]),
    )
    .unwrap();
    let values = nums(&m["steps"])
        .into_iter()
        .map(|s| lfo.advance(s))
        .collect();
    (values, lfo)
}

#[test]
fn waves_match_python() {
    let blob = Blob::open("lfo");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("lfo/"))
        .collect();
    assert_eq!(labels.len(), 7);
    for label in &labels {
        let m = blob.meta(label);
        let (got, lfo) = run(m);
        assert_close(&got, blob.get(label), label);
        assert!(
            (lfo.phase - num(&m["phase_end"])).abs() <= 1e-12,
            "{label} phase"
        );
    }
}

#[test]
fn nonfinite_inputs_are_finite_and_bounded() {
    for wave in 0..5 {
        let mut lfo = Lfo::new(SR, 3);
        lfo.set(f64::NAN, wave, f64::INFINITY, f64::NAN).unwrap();
        for s in [f64::NAN, f64::INFINITY, -1.0, 0.01, 1e300] {
            let v = lfo.advance(s);
            // depth = inf is not finite, so it falls back to 0 like Python.
            assert!(v.is_finite() && v.abs() <= 0.0, "wave {wave}: {v}");
        }
        let mut lfo = Lfo::new(SR, 3);
        lfo.set(f64::INFINITY, wave, 0.7, 0.0).unwrap();
        for s in [0.01, f64::INFINITY, 1e300] {
            let v = lfo.advance(s);
            assert!(v.is_finite() && v.abs() <= 0.7, "wave {wave}: {v}");
            assert!((0.0..1.0).contains(&lfo.phase));
        }
    }
}

#[test]
fn invalid_wave_rejected_without_state_change() {
    let mut a = Lfo::new(SR, 1);
    a.set(2.0, 1, 0.5, 0.1).unwrap();
    let mut b = Lfo::new(SR, 1);
    b.set(2.0, 1, 0.5, 0.1).unwrap();
    assert!(b.set(9.0, 5, 1.0, 0.0).is_err());
    assert!(b.set(9.0, -1, 1.0, 0.0).is_err());
    for _ in 0..50 {
        assert_eq!(a.advance(0.01), b.advance(0.01));
    }
}

#[test]
fn sample_and_hold_changes_only_at_wrap() {
    let mut lfo = Lfo::new(SR, 5);
    lfo.set(10.0, 4, 1.0, 0.0).unwrap();
    let mut prev_phase = lfo.phase;
    let mut prev = lfo.advance(0.0);
    for _ in 0..500 {
        let v = lfo.advance(0.013);
        let wrapped = lfo.phase < prev_phase + 0.13 - 1e-9;
        if !wrapped {
            assert_eq!(v, prev);
        }
        prev_phase = lfo.phase;
        prev = v;
    }
}
