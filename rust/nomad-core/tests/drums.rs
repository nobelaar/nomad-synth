mod common;

use common::{Blob, assert_checksums, assert_close, drums_render, num};
use nomad_core::clock::Clock;
use nomad_core::drum_synth::render_one_shot;
use nomad_core::drums::{DrumMachine, ValueEffect};
use nomad_core::params::{DRUM_PARAMS, INSTRUMENTS, find};

const SR: f64 = 48000.0;
const ID: u64 = 1;

fn pid(name: &str) -> usize {
    DRUM_PARAMS.iter().position(|s| s.name == name).unwrap()
}

fn track(name: &str) -> usize {
    INSTRUMENTS.iter().position(|t| *t == name).unwrap()
}

/// Apply a value like the exporter: bpm goes to the clock, tone changes regenerate.
fn set(dm: &mut DrumMachine, clock: &mut Clock, name: &str, v: f64) -> ValueEffect {
    let effect = dm.set_value(pid(name), v);
    if let ValueEffect::Bpm(b) = effect {
        clock.bpm = b.clamp(20.0, 300.0);
    }
    effect
}

fn run(m: &serde_json::Value) -> Vec<f64> {
    let mut dm = DrumMachine::new(SR);
    let mut clock = Clock::new(SR);
    clock.bpm = dm.values[pid("bpm")];
    let block = m["block"].as_u64().unwrap() as usize;
    let mut out = Vec::new();
    for c in m["commands"].as_array().unwrap() {
        let c = c.as_array().unwrap();
        let s = |k: usize| c[k].as_str().unwrap();
        match s(0) {
            "set" => {
                set(&mut dm, &mut clock, s(1), num(&c[2]));
            }
            "bank" => {
                let t = c[1].as_u64().unwrap() as usize;
                let (tune, decay) = (dm.values[3 * t], dm.values[3 * t + 1]);
                dm.set_bank(t, render_one_shot(t, SR, tune, decay));
            }
            "start" => clock.start(),
            "trigger" => dm.trigger(track(s(1)), num(&c[2])),
            "step" => dm.set_step(
                track(s(1)),
                c[2].as_u64().unwrap() as usize,
                c[3].as_u64().unwrap() as u8,
            ),
            "clear" => dm.clear_pattern(),
            "stop_all" => dm.stop_all(&mut clock),
            "render" => {
                let mut left = c[1].as_u64().unwrap() as usize;
                while left > 0 {
                    let mut b = vec![0.0; block.min(left)];
                    drums_render(&mut dm, &mut clock, ID, &mut b);
                    left -= b.len();
                    out.extend(b);
                }
            }
            other => panic!("unknown command {other}"),
        }
    }
    out
}

#[test]
fn scenarios_match_python() {
    let mut count = 0;
    for name in ["drums", "drums_more"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            let m = blob.meta(&label);
            let full = run(m);
            assert_eq!(full.len() as u64, m["len"].as_u64().unwrap(), "{label}");
            let decim = m["decim"].as_u64().unwrap() as usize;
            let got: Vec<f64> = full.iter().copied().step_by(decim).collect();
            assert_close(&got, blob.get(&label), &label);
            assert_checksums(&full, m, &label);
            count += 1;
        }
    }
    assert_eq!(count, 7);
}

#[test]
fn default_pattern_matches_python() {
    let blob = Blob::open("drums");
    let want: Vec<Vec<u8>> =
        serde_json::from_value(blob.meta("drums/default_2bars")["default_pattern"].clone())
            .unwrap();
    let dm = DrumMachine::new(SR);
    assert_eq!(
        dm.steps.iter().map(|r| r.to_vec()).collect::<Vec<_>>(),
        want
    );
}

#[test]
fn stop_all_fades_to_silence() {
    let mut dm = DrumMachine::new(SR);
    let mut clock = Clock::new(SR);
    clock.start();
    let mut b = vec![0.0; 2048];
    drums_render(&mut dm, &mut clock, ID, &mut b);
    dm.stop_all(&mut clock);
    assert!(!clock.running && clock.pos == 0.0);
    let mut fade = vec![0.0; 600];
    drums_render(&mut dm, &mut clock, ID, &mut fade);
    // 5 ms = 240 frames: a ramp down, then exact silence.
    assert!(
        fade[..240]
            .windows(2)
            .all(|w| w[1].abs() <= w[0].abs() + 0.2)
    );
    assert!(fade[240..].iter().all(|&x| x == 0.0));
    let mut after = vec![0.0; 4096];
    drums_render(&mut dm, &mut clock, ID, &mut after);
    assert!(after.iter().all(|&x| x == 0.0));
}

#[test]
fn set_value_effects() {
    let mut dm = DrumMachine::new(SR);
    assert_eq!(
        dm.set_value(pid("kick_tune"), 0.4),
        ValueEffect::Regenerate(0)
    );
    assert_eq!(
        dm.set_value(pid("ohat_decay"), 0.4),
        ValueEffect::Regenerate(5)
    );
    assert_eq!(dm.set_value(pid("bpm"), 999.0), ValueEffect::Bpm(180.0));
    assert_eq!(dm.set_value(pid("swing"), 0.3), ValueEffect::None);
    assert_eq!(dm.grid.swing, 0.3);
    assert_eq!(
        dm.set_value(pid("snare_level"), f64::NAN),
        ValueEffect::None
    );
    assert_eq!(
        dm.values[pid("snare_level")],
        find(&DRUM_PARAMS, "snare_level").unwrap().default
    );
    assert_eq!(dm.set_value(99, 1.0), ValueEffect::None);
}

#[test]
fn step_levels_clamped() {
    let mut dm = DrumMachine::new(SR);
    dm.set_step(2, 3, 9);
    assert_eq!(dm.steps[2][3], 2);
    dm.set_step(2, 3, 0);
    assert_eq!(dm.steps[2][3], 0);
    dm.set_step(9, 3, 1); // bad track: ignored
    dm.set_step(2, 99, 1); // bad step: ignored
    dm.clear_pattern();
    assert!(dm.steps.iter().flatten().all(|&s| s == 0));
}
