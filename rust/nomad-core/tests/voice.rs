mod common;

use common::{Blob, assert_close, num, params_from};
use nomad_core::mods::Mods;
use nomad_core::params::{ParamValues, default_values, pid};
use nomad_core::voice::Voice;

const SR: f64 = 48000.0;

fn mods_from(v: &serde_json::Value) -> Mods {
    let g = |k: &str| v.get(k).map(num).unwrap_or(0.0);
    Mods {
        pitch: g("pitch"),
        pitch1: g("pitch1"),
        pitch2: g("pitch2"),
        amp: g("amp"),
        cutoff: g("cutoff"),
        pw1: g("pw1"),
        pw2: g("pw2"),
        bend: g("bend"),
    }
}

fn render(v: &mut Voice, p: &ParamValues, m: &Mods, n: usize, block: usize) -> Vec<f64> {
    let mut all = Vec::with_capacity(n);
    let mut left = n;
    while left > 0 {
        let mut buf = vec![0.0; block.min(left)];
        v.render(&mut buf, p, m);
        left -= buf.len();
        all.extend(buf);
    }
    all
}

fn replay(m: &serde_json::Value) -> (Vec<f64>, Voice) {
    let p = params_from(&m["params"]);
    let mods = mods_from(&m["mods"]);
    let block = m["block"].as_u64().unwrap() as usize;
    let mut v = Voice::new(SR);
    let mut all = Vec::new();
    for cmd in m["commands"].as_array().unwrap() {
        let c = cmd.as_array().unwrap();
        let note = || c[1].as_i64().unwrap() as i32;
        match c[0].as_str().unwrap() {
            "on" => v.note_on(note(), num(&c[2]), c.get(3).map(num)),
            "off" => v.note_off(),
            "steal" => v.begin_steal(note(), num(&c[2])),
            "target" => v.set_target(note()),
            "run" => all.extend(render(
                &mut v,
                &p,
                &mods,
                c[1].as_u64().unwrap() as usize,
                block,
            )),
            other => panic!("unknown command {other}"),
        }
    }
    (all, v)
}

#[test]
fn scenarios_match_python() {
    let mut count = 0;
    for name in ["voice", "voice_glide"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            let m = blob.meta(&label);
            let (got, v) = replay(m);
            // The plan allows 1e-7 for whole voices; the port holds the strict 1e-9.
            assert_close(&got, blob.get(&label), &label);
            assert_eq!(v.is_active(), m["active_end"].as_bool().unwrap(), "{label}");
            count += 1;
        }
    }
    assert_eq!(count, 15);
}

#[test]
fn silent_until_note_on() {
    let mut v = Voice::new(SR);
    assert!(!v.is_active());
    let out = render(&mut v, &default_values(), &Mods::default(), 512, 256);
    assert!(out.iter().all(|&x| x == 0.0));
}

#[test]
fn idle_after_release() {
    let mut v = Voice::new(SR);
    let p = default_values();
    v.note_on(60, 0.8, None);
    render(&mut v, &p, &Mods::default(), 4800, 256);
    v.note_off();
    // Default release is 400 ms: well inside one second.
    render(&mut v, &p, &Mods::default(), 48000, 256);
    assert!(!v.is_active());
}

#[test]
fn extreme_mods_stay_finite_and_bounded() {
    let mut p = default_values();
    p[pid::FILT_RESO] = 1.0;
    p[pid::FILT_CUTOFF] = f64::NAN;
    for m in [
        Mods {
            pitch: 1000.0,
            cutoff: f64::NAN,
            amp: -5.0,
            ..Mods::default()
        },
        Mods {
            pitch: -1000.0,
            cutoff: 1e9,
            amp: 5.0,
            bend: f64::INFINITY,
            ..Mods::default()
        },
        Mods {
            pw1: f64::NAN,
            pw2: 1e9,
            pitch1: f64::NEG_INFINITY,
            ..Mods::default()
        },
    ] {
        let mut v = Voice::new(SR);
        v.note_on(60, 1.0, None);
        for x in render(&mut v, &p, &m, 4096, 256) {
            assert!(x.is_finite() && x.abs() < 10.0, "{m:?}: {x}");
        }
    }
}

#[test]
fn velocity_is_clamped() {
    let p = default_values();
    let play = |vel: f64| {
        let mut v = Voice::new(SR);
        v.note_on(60, vel, None);
        render(&mut v, &p, &Mods::default(), 1024, 256)
    };
    assert_eq!(play(7.0), play(1.0));
    assert_eq!(play(f64::NAN), play(0.0));
    assert!(play(0.0).iter().all(|&x| x == 0.0));
}

#[test]
fn glide_zero_equals_no_start() {
    let p = default_values();
    assert_eq!(p[pid::GLIDE], 0.0);
    let play = |start: Option<f64>| {
        let mut v = Voice::new(SR);
        v.note_on(72, 0.8, start);
        render(&mut v, &p, &Mods::default(), 2048, 256)
    };
    assert_eq!(play(Some(60.0)), play(None));
}
