mod common;

use common::{Blob, assert_close, num};
use nomad_core::drum_synth::{butter1_bandpass, lfilter, render_one_shot};
use nomad_core::params::INSTRUMENTS;

fn shot_blobs() -> Vec<Blob> {
    let mut v: Vec<Blob> = INSTRUMENTS
        .iter()
        .map(|k| Blob::open(&format!("drum_shots_{k}")))
        .collect();
    v.push(Blob::open("drum_shots_44k"));
    v
}

#[test]
fn butter_matches_scipy() {
    let blob = Blob::open("drum_shots");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("butter/"))
        .collect();
    assert_eq!(labels.len(), 6);
    for label in &labels {
        let m = blob.meta(label);
        let (b, a) = butter1_bandpass(num(&m["low"]), num(&m["high"]));
        assert_close(&[b, a].concat(), blob.get(label), label);
    }
}

#[test]
fn lfilter_matches_scipy() {
    let blob = Blob::open("drum_shots");
    let m = blob.meta("lfilter/basic");
    let (b, a): (Vec<f64>, Vec<f64>) = (common::nums(&m["b"]), common::nums(&m["a"]));
    let mut x = blob.get("lfilter/input").to_vec();
    lfilter(&b, &a, &mut x);
    assert_close(&x, blob.get("lfilter/basic"), "lfilter");
}

#[test]
fn shots_match_python() {
    let mut count = 0;
    for blob in shot_blobs() {
        for label in blob.labels() {
            let m = blob.meta(&label);
            let kind = m["kind"].as_u64().unwrap() as usize;
            let y = render_one_shot(kind, num(&m["sr"]), num(&m["tune"]), num(&m["decay"]));
            assert_eq!(y.len() as u64, m["len"].as_u64().unwrap(), "{label} len");
            let (head, decim) = (
                m["head"].as_u64().unwrap() as usize,
                m["decim"].as_u64().unwrap() as usize,
            );
            let got: Vec<f64> = y[..head]
                .iter()
                .chain(y[head..].iter().step_by(decim))
                .map(|&v| f64::from(v))
                .collect();
            let want = blob.get(&label);
            assert_eq!(got.len(), want.len(), "{label}");
            for (i, (a, b)) in got.iter().zip(want).enumerate() {
                // f32 output: one float32 ulp near 0.9 is ~6e-8.
                assert!((a - b).abs() <= 1e-6, "{label}[{i}]: {a} vs {b}");
            }
            let full: Vec<f64> = y.iter().map(|&v| f64::from(v)).collect();
            let sum: f64 = full.iter().sum();
            let sumsq: f64 = full.iter().map(|v| v * v).sum();
            for (g, key) in [(sum, "sum"), (sumsq, "sumsq")] {
                let w = num(&m[key]);
                assert!(
                    (g - w).abs() <= 1e-6 * w.abs().max(1.0),
                    "{label} {key}: {g} vs {w}"
                );
            }
            count += 1;
        }
    }
    assert_eq!(count, 36);
}

#[test]
fn shots_are_finite_peak_0_9() {
    for kind in 0..6 {
        for (tune, decay) in [
            (0.0, 0.5),
            (f64::NAN, f64::INFINITY),
            (-1.0, 0.0),
            (1.0, 1.0),
            (5.0, -3.0),
        ] {
            let y = render_one_shot(kind, 48000.0, tune, decay);
            assert!(y.iter().all(|v| v.is_finite()));
            let peak = y.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
            assert!((f64::from(peak) - 0.9).abs() <= 1e-6, "kind {kind}: {peak}");
        }
    }
}

#[test]
fn deterministic() {
    for kind in 0..6 {
        assert_eq!(
            render_one_shot(kind, 48000.0, 0.2, 0.4),
            render_one_shot(kind, 48000.0, 0.2, 0.4)
        );
    }
}

/// Stronger than the plan's 1e-6: the engine golden (Task 5) needs exact shots.
#[test]
fn stored_samples_are_bit_exact() {
    for blob in shot_blobs() {
        for label in blob.labels() {
            let m = blob.meta(&label);
            let y = render_one_shot(
                m["kind"].as_u64().unwrap() as usize,
                num(&m["sr"]),
                num(&m["tune"]),
                num(&m["decay"]),
            );
            let (head, decim) = (
                m["head"].as_u64().unwrap() as usize,
                m["decim"].as_u64().unwrap() as usize,
            );
            let got: Vec<f64> = y[..head]
                .iter()
                .chain(y[head..].iter().step_by(decim))
                .map(|&v| f64::from(v))
                .collect();
            assert_eq!(got, blob.get(&label), "{label}");
        }
    }
}
