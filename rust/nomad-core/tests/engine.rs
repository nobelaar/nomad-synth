mod common;

use common::{Blob, assert_close, assert_engine_run, ints, run_engine_ops};
use nomad_core::engine::Engine;
use nomad_core::params::{DRUM_PARAMS, pid};

const SR: f64 = 48000.0;
const SYNTH: u32 = 1;
const DRUMS: u32 = 2;

fn render(e: &mut Engine, sizes: &[usize]) -> Vec<f64> {
    let mut all = Vec::new();
    for &n in sizes {
        let mut o = vec![0.0; n];
        e.render(&mut o);
        all.extend(o);
    }
    all
}

#[test]
fn default_engine_matches_golden() {
    // tests/test_engine_mixer.py::test_default_level_is_bit_identical_to_before
    let blob = Blob::open("engine");
    let sizes = ints(&blob.meta("golden")["sizes"]);
    let mut e = Engine::new(SR, 8);
    let bpm = DRUM_PARAMS.iter().position(|s| s.name == "bpm").unwrap();
    e.set_drum_value(DRUMS, bpm, 120.0);
    assert!(e.start(DRUMS));
    e.note_on(60, 0.8);
    e.note_on(67, 0.6);
    let mut out = Vec::new();
    for (k, &n) in sizes.iter().enumerate() {
        out.extend(render(&mut e, &[n]));
        if k == 3 {
            e.note_off(60);
            e.note_on(64, 1.0);
        }
    }
    assert_eq!(out.len(), 8633);
    assert_close(&out, blob.get("golden"), "golden");
}

#[test]
fn scenarios_match_python() {
    let blob = Blob::open("engine");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("mix/"))
        .collect();
    assert_eq!(labels.len(), 5);
    for label in &labels {
        let mut e = Engine::new(SR, 8);
        let r = run_engine_ops(&mut e, &blob.meta(label)["commands"]);
        assert_engine_run(&r, &blob, label);
    }
}

#[test]
fn mute_ramps_without_click_and_keeps_clock_running() {
    // tests/test_engine_mixer.py::test_mute_ramps_without_click_and_ends_silent, plus the clock.
    let (mut reference, mut e) = (Engine::new(SR, 8), Engine::new(SR, 8));
    for x in [&mut reference, &mut e] {
        x.note_on(60, 0.8);
        x.start(SYNTH);
        render(x, &[2048]);
    }
    e.set_mute(SYNTH, true);
    let (y, r) = (render(&mut e, &[600]), render(&mut reference, &[600]));
    let max_step = |v: &[f64]| {
        v.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max)
    };
    assert!(max_step(&y[..256]) <= max_step(&r[..256]) * 1.05);
    assert!(y[300..].iter().all(|&x| x == 0.0));
    assert!(e.clock(SYNTH).unwrap().running);
}

#[test]
fn level_clamps() {
    let mut e = Engine::new(SR, 8);
    e.set_level(SYNTH, 7.0);
    assert_eq!(e.level(SYNTH), Some(1.0));
    e.set_level(SYNTH, -1.0);
    assert_eq!(e.level(SYNTH), Some(0.0));
    e.set_level(SYNTH, f64::NAN);
    assert_eq!(e.level(SYNTH), Some(1.0));
    assert_eq!(e.level(99), None);
}

#[test]
fn commands_fail_for_unknown_id() {
    let mut e = Engine::new(SR, 8);
    assert!(!e.start(99) && !e.stop(99) && !e.set_bpm(99, 120.0));
    assert!(e.synth_mut(DRUMS).is_none() && e.drums_mut(SYNTH).is_none());
    assert!(e.clock(99).is_none() && e.effective_clock(99).is_none());
    assert!(!e.set_target_synth(DRUMS) && e.set_target_synth(SYNTH));
    e.set_mute(99, true);
    e.set_level(99, 0.5);
    let _ = pid::COUNT;
}

#[test]
fn each_root_clock_advances_once_per_chunk() {
    // Both own clocks start together at the same tempo: if either advanced more
    // than once per chunk their positions would drift apart.
    for sizes in [vec![37; 40], vec![512; 6]] {
        let mut e = Engine::new(SR, 8);
        e.set_bpm(SYNTH, 120.0);
        e.set_bpm(DRUMS, 120.0);
        e.start(SYNTH);
        e.start(DRUMS);
        let total: usize = sizes.iter().sum();
        render(&mut e, &sizes);
        let (a, b) = (e.clock(SYNTH).unwrap().pos, e.clock(DRUMS).unwrap().pos);
        assert_eq!(a, b);
        assert!((a - total as f64 * 120.0 / (60.0 * SR)).abs() < 1e-9);
    }
}

#[test]
fn tap_copies_one_instance_and_leaves_the_mix_alone() {
    let (mut a, mut b) = (Engine::new(SR, 8), Engine::new(SR, 8));
    for e in [&mut a, &mut b] {
        e.note_on(57, 1.0);
    }
    a.set_tap(Some(SYNTH));
    let (mut out_a, mut tap) = (vec![0.0; 4096], vec![1.0; 4096]);
    let mut out_b = vec![0.0; 4096];
    a.render_with_tap(&mut out_a, &mut tap);
    b.render(&mut out_b);
    assert_eq!(out_a, out_b, "tapping does not change the sound");
    assert!(
        tap.iter().any(|&x| x.abs() > 1e-3),
        "the synth is in the tap"
    );
    a.set_tap(Some(DRUMS));
    a.render_with_tap(&mut out_a, &mut tap);
    assert!(tap.iter().all(|&x| x == 0.0), "idle drums: silent tap");
}
