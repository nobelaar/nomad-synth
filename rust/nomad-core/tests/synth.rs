mod common;

use common::{Blob, assert_close, num};
use nomad_core::params::{SYNTH_PARAMS, find, pid};
use nomad_core::synth::{CONTROL_BLOCK, Control, Synth, is_smoothed};

const SR: f64 = 48000.0;

fn id(name: &str) -> usize {
    SYNTH_PARAMS
        .iter()
        .position(|s| s.name == name)
        .unwrap_or_else(|| panic!("{name}"))
}

fn replay(commands: &serde_json::Value) -> Vec<f64> {
    let mut s = Synth::new(SR, 8);
    let mut out = Vec::new();
    for cmd in commands.as_array().unwrap() {
        let c = cmd.as_array().unwrap();
        match c[0].as_str().unwrap() {
            "param" => s.set_param(id(c[1].as_str().unwrap()), num(&c[2])),
            "ctl" => {
                let ctl = match c[1].as_str().unwrap() {
                    "bend" => Control::Bend,
                    _ => Control::ModWheel,
                };
                s.set_control(ctl, num(&c[2]));
            }
            "on" => s.note_on(c[1].as_i64().unwrap(), num(&c[2])),
            "off" => s.note_off(c[1].as_i64().unwrap()),
            "panic" => s.panic(),
            "run" => {
                let mut b = vec![0.0; c[1].as_u64().unwrap() as usize];
                s.render_block(&mut b);
                out.extend(b);
            }
            other => panic!("unknown command {other}"),
        }
    }
    out
}

#[test]
fn scenarios_match_python() {
    let mut count = 0;
    for name in ["synth", "synth_more"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            // The plan allows 1e-7 for the whole synth; the port holds the strict 1e-9.
            assert_close(
                &replay(&blob.meta(&label)["commands"]),
                blob.get(&label),
                &label,
            );
            count += 1;
        }
    }
    assert_eq!(count, 7);
}

#[test]
fn smoothed_set_matches_python() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/synth_sets.json");
    let sets: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let names = |k: &str| -> Vec<String> {
        sets[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let (smoothed, immediate) = (names("smoothed"), names("immediate"));
    assert_eq!(smoothed.len() + immediate.len(), 62);
    for n in &smoothed {
        assert!(is_smoothed(id(n)), "{n} should be smoothed");
    }
    for n in &immediate {
        assert!(!is_smoothed(id(n)), "{n} should be immediate");
    }
    assert!(!is_smoothed(pid::MASTER_VOLUME));
}

#[test]
fn set_param_clamps_and_ignores_master() {
    let mut s = Synth::new(SR, 8);
    s.set_param(pid::FILT_CUTOFF, 1e9);
    assert_eq!(s.values()[pid::FILT_CUTOFF], 18000.0);
    s.set_param(pid::FILT_RESO, f64::NAN);
    assert_eq!(
        s.values()[pid::FILT_RESO],
        find(&SYNTH_PARAMS, "filt_reso").unwrap().default
    );
    s.set_param(pid::MASTER_VOLUME, 0.1);
    assert_eq!(s.values()[pid::MASTER_VOLUME], 0.7);
    s.set_param(63, 1.0);
    s.set_param(usize::MAX, 1.0);
}

#[test]
fn set_control_clamps() {
    // Bend 5.0 clamps to 1.0 and NaN to 0: compare against explicit values.
    let play = |bend: f64| {
        let mut s = Synth::new(SR, 8);
        s.set_control(Control::Bend, bend);
        s.note_on(60, 0.8);
        let mut out = vec![0.0; 2048];
        for b in out.chunks_mut(CONTROL_BLOCK) {
            s.render_block(b);
        }
        out
    };
    assert_eq!(play(5.0), play(1.0));
    assert_eq!(play(f64::NAN), play(0.0));
    assert_ne!(play(1.0), play(0.0));
}

#[test]
fn render_adds_into_buffer() {
    let render = |init: f64| {
        let mut s = Synth::new(SR, 8);
        s.note_on(60, 0.8);
        let mut b = vec![init; CONTROL_BLOCK];
        s.render_block(&mut b);
        b
    };
    let (zero, one) = (render(0.0), render(1.0));
    for (a, b) in zero.iter().zip(&one) {
        assert_eq!(a + 1.0, *b);
    }
}

#[test]
fn is_live_tracks_note_on_off() {
    let mut s = Synth::new(SR, 8);
    s.note_on(60, 0.8);
    s.note_on(64, 0.8);
    s.note_on(200, 0.8);
    assert!(s.is_live(60) && s.is_live(64) && !s.is_live(61) && !s.is_live(200));
    s.note_off(60);
    assert!(!s.is_live(60) && s.is_live(64));
    s.panic();
    assert!(!s.is_live(64));
}

#[test]
fn panic_goes_silent() {
    let mut s = Synth::new(SR, 8);
    for n in [48, 55, 60, 64, 67] {
        s.note_on(n, 1.0);
    }
    let mut b = [0.0; CONTROL_BLOCK];
    for _ in 0..20 {
        s.render_block(&mut b);
    }
    s.panic();
    for _ in 0..4 {
        b = [0.0; CONTROL_BLOCK];
        s.render_block(&mut b);
    }
    b = [0.0; CONTROL_BLOCK];
    s.render_block(&mut b);
    assert!(b.iter().all(|&x| x == 0.0));
}
