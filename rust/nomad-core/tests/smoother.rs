mod common;

use common::{close, load};
use nomad_core::smoother::Smoother;

#[test]
fn matches_python_traces() {
    for c in load().smoother {
        let mut s = Smoother::new(c.start, c.sample_rate, c.time_ms);
        s.set(c.target);
        for (&n, &v) in c.steps.iter().zip(&c.values) {
            assert!(close(s.advance(n), v), "advance({n})");
        }
    }
}

#[test]
fn zero_or_negative_frames_do_not_move() {
    let mut s = Smoother::new(0.0, 48000.0, 15.0);
    s.set(1.0);
    let v = s.advance(64);
    assert_eq!(s.advance(0), v);
    assert_eq!(s.advance(-5), v);
    assert_eq!(s.value, v);
}

#[test]
fn zero_time_does_not_nan() {
    let mut s = Smoother::new(0.25, 44100.0, 0.0);
    s.set(0.75);
    let v = s.advance(1);
    assert!(v.is_finite() && (0.25..=0.75).contains(&v));
}

#[test]
fn never_overshoots() {
    let mut s = Smoother::new(0.0, 48000.0, 15.0);
    s.set(1.0);
    let mut prev = s.value;
    for _ in 0..1000 {
        let v = s.advance(64);
        assert!(v <= 1.0 && v >= prev);
        prev = v;
    }
}
