mod common;

use common::{Blob, assert_close, ints, num};
use nomad_core::filter::{Ladder4Pole, ladder_coeffs};

const SR: f64 = 48000.0;

fn run(f: &mut Ladder4Pole, buf: &mut [f64], grains: &[usize]) {
    let mut pos = 0;
    for &n in grains {
        f.process(&mut buf[pos..pos + n]);
        pos += n;
    }
}

#[test]
fn coeffs_match_python() {
    let blob = Blob::open("filter");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("coeffs/"))
        .collect();
    assert_eq!(labels.len(), 12);
    for label in &labels {
        let m = blob.meta(label);
        let (b, a) = ladder_coeffs(num(&m["fc"]), num(&m["k"]), SR);
        assert_close(&[b, a].concat(), blob.get(label), label);
    }
}

#[test]
fn run_matches_python() {
    let blob = Blob::open("filter");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("run/") || l.starts_with("set/"))
        .collect();
    assert_eq!(labels.len(), 16);
    for label in &labels {
        let m = blob.meta(label);
        let mut f = Ladder4Pole::new(SR);
        f.set(num(&m["fc"]), num(&m["res"]));
        let mut buf = blob.get(m["input"].as_str().unwrap()).to_vec();
        run(&mut f, &mut buf, &ints(&m["grains"]));
        assert_close(&buf, blob.get(label), label);
    }
}

#[test]
fn self_oscillation_stays_bounded() {
    let mut f = Ladder4Pole::new(SR);
    f.set(1000.0, 1.0);
    let mut buf = vec![0.0_f64; 100_000];
    buf[0] = 1.0;
    // The engine feeds grains of 16 when k > 3.2; the state clamp runs per call.
    for g in buf.chunks_mut(16) {
        f.process(g);
    }
    assert!(buf.iter().all(|y| y.is_finite() && y.abs() < 50.0));
}

#[test]
fn nan_settings_stay_finite() {
    let mut f = Ladder4Pole::new(SR);
    f.set(f64::NAN, f64::NAN);
    let mut buf: Vec<f64> = (0..256)
        .map(|i| ((i * 7919) % 13) as f64 / 13.0 - 0.5)
        .collect();
    f.process(&mut buf);
    assert!(buf.iter().all(|y| y.is_finite()));
}

#[test]
fn long_buffer_equals_chunks() {
    // Below k = 3.2 there is no per-call state clamp, so chunking is invisible.
    let x: Vec<f64> = (0..1000)
        .map(|i| ((i * 7919) % 101) as f64 / 101.0 - 0.5)
        .collect();
    let mut one = x.clone();
    let mut f = Ladder4Pole::new(SR);
    f.set(2000.0, 0.5);
    f.process(&mut one);
    let mut chunks = x.clone();
    let mut f = Ladder4Pole::new(SR);
    f.set(2000.0, 0.5);
    for g in chunks.chunks_mut(16) {
        f.process(g);
    }
    for (a, b) in one.iter().zip(&chunks) {
        assert!((a - b).abs() <= 1e-12);
    }
}

#[test]
fn reset_clears_state() {
    let x = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut f = Ladder4Pole::new(SR);
    f.set(1000.0, 0.3);
    let mut first = x;
    f.process(&mut first);
    f.reset();
    let mut again = x;
    f.process(&mut again);
    assert_eq!(first, again);
}
