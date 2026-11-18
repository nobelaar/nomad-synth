mod common;

use common::{Blob, assert_close, ints, num};
use nomad_core::oscillator::{Oscillator, Wave};

const SR: f64 = 48000.0;

fn osc(wave: Wave) -> Oscillator {
    let mut o = Oscillator::new(SR);
    o.wave = wave;
    o
}

/// Render contiguous blocks (each starting from zeros) and concatenate them.
fn render(o: &mut Oscillator, freq: f64, pw: f64, blocks: &[usize]) -> Vec<f64> {
    let mut all = Vec::new();
    for &n in blocks {
        let mut buf = vec![0.0; n];
        o.render_into(&mut buf, freq, pw, None);
        all.extend(buf);
    }
    all
}

fn check_prefix(prefix: &str) -> usize {
    let blob = Blob::open("oscillator");
    let mut count = 0;
    for label in blob.labels().iter().filter(|l| l.starts_with(prefix)) {
        let m = blob.meta(label);
        let wave = Wave::from_index(m["wave"].as_i64().unwrap()).unwrap();
        let mut o = osc(wave);
        let got = render(&mut o, num(&m["freq"]), num(&m["pw"]), &ints(&m["blocks"]));
        assert_close(&got, blob.get(label), label);
        let want_phase = num(&m["phase_end"]);
        assert!(
            (o.phase - want_phase).abs() <= 1e-12,
            "{label}: phase {} vs {want_phase}",
            o.phase
        );
        count += 1;
    }
    count
}

#[test]
fn free_waves_match_python() {
    assert_eq!(check_prefix("free/"), 48);
}

#[test]
fn edge_cases_match_python() {
    assert_eq!(check_prefix("edge/"), 5);
}

#[test]
fn wave_from_index() {
    assert_eq!(Wave::from_index(0), Some(Wave::Sine));
    assert_eq!(Wave::from_index(3), Some(Wave::Square));
    assert_eq!(Wave::from_index(4), None);
    assert_eq!(Wave::from_index(-1), None);
}

#[test]
fn no_nan_for_any_wave_at_non_finite_params() {
    let freqs = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, 1e9, -1e9];
    let pws = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 2.0];
    for i in 0..4 {
        for &f in &freqs {
            for &pw in &pws {
                let mut o = osc(Wave::from_index(i).unwrap());
                for x in render(&mut o, f, pw, &[256, 7]) {
                    assert!(
                        x.is_finite() && x.abs() <= 2.5,
                        "wave {i} f {f} pw {pw}: {x}"
                    );
                }
                assert!(o.phase.is_finite() && (0.0..1.0).contains(&o.phase));
            }
        }
    }
}

#[test]
fn block_split_is_invisible() {
    for i in 0..4 {
        let w = Wave::from_index(i).unwrap();
        let one = render(&mut osc(w), 5200.0, 0.3, &[500]);
        let two = render(&mut osc(w), 5200.0, 0.3, &[256, 244]);
        for (a, b) in one.iter().zip(&two) {
            assert!((a - b).abs() <= 1e-12, "wave {i}");
        }
    }
}

#[test]
fn sine_matches_libm() {
    let got = render(&mut osc(Wave::Sine), 1000.0, 0.5, &[480]);
    for (n, x) in got.iter().enumerate() {
        let want = (2.0 * std::f64::consts::PI * n as f64 * 1000.0 / SR).sin();
        assert!((x - want).abs() <= 1e-9, "n {n}");
    }
}

#[test]
fn render_adds_into_buffer() {
    let reference = render(&mut osc(Wave::Saw), 440.0, 0.5, &[256]);
    let mut buf = vec![1.0; 256];
    osc(Wave::Saw).render_into(&mut buf, 440.0, 0.5, None);
    for (a, b) in buf.iter().zip(&reference) {
        assert_eq!(*a, 1.0 + b);
    }
}
