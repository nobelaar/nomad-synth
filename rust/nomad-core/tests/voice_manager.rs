mod common;

use common::{Blob, assert_close, num};
use nomad_core::mods::Mods;
use nomad_core::params::{ParamValues, default_values, pid};
use nomad_core::voice::Voice;
use nomad_core::voice_manager::VoiceManager;

const SR: f64 = 48000.0;

fn render(vm: &mut VoiceManager, n: usize, p: &ParamValues, amp: Option<f64>) -> Vec<f64> {
    let mut out = vec![0.0; n];
    for chunk in out.chunks_mut(16) {
        match amp {
            Some(a) => vm.render(chunk, p, &Mods::default(), |_: &Voice, m: &Mods| Mods {
                amp: a,
                ..*m
            }),
            None => vm.render(chunk, p, &Mods::default(), |_: &Voice, m: &Mods| *m),
        }
    }
    out
}

fn sorted(mut v: Vec<i32>) -> Vec<i32> {
    v.sort();
    v
}

#[test]
fn scenarios_match_python() {
    let blob = Blob::open("voice_manager");
    assert_eq!(blob.labels().len(), 10);
    let p = default_values();
    for label in blob.labels() {
        let m = blob.meta(&label);
        let amp = m["resolve_amp"].as_f64();
        let mut vm = VoiceManager::new(SR, 8);
        let (mut out, mut checks) = (Vec::new(), Vec::new());
        for cmd in m["commands"].as_array().unwrap() {
            let c = cmd.as_array().unwrap();
            let int = |k: usize| c[k].as_i64().unwrap();
            match c[0].as_str().unwrap() {
                "on" => vm.note_on(int(1), num(&c[2])),
                "off" => vm.note_off(int(1)),
                "poly" => vm.set_polyphony(int(1)),
                "glide" => vm.set_glide(num(&c[1])),
                "panic" => vm.panic(),
                "run" => {
                    out.extend(render(&mut vm, int(1) as usize, &p, amp));
                    checks.push(sorted(vm.active_notes()));
                }
                other => panic!("unknown command {other}"),
            }
        }
        // The plan allows 1e-7 here; the port holds the strict 1e-9.
        assert_close(&out, blob.get(&label), &label);
        let want: Vec<Vec<i32>> = serde_json::from_value(m["checks"].clone()).unwrap();
        assert_eq!(checks, want, "{label}");
    }
}

#[test]
fn steal_has_no_step_discontinuity() {
    // Port of tests/test_voice_manager.py::test_steal_has_no_step_discontinuity.
    let mut vm = VoiceManager::new(SR, 8);
    for n in 60..68 {
        vm.note_on(n, 1.0);
    }
    render(&mut vm, 8192, &default_values(), None);
    let mut p = default_values();
    p[pid::OSC1_WAVE] = 0.0;
    p[pid::OSC2_LEVEL] = 0.0;
    p[pid::AMP_ATTACK] = 1.0;
    p[pid::AMP_SUSTAIN] = 1.0;
    let mut out = vec![0.0_f64; 2400];
    for (k, chunk) in out.chunks_mut(16).enumerate() {
        if k * 16 == 480 {
            vm.note_on(80, 1.0);
        }
        vm.render(chunk, &p, &Mods::default(), |_: &Voice, m: &Mods| *m);
    }
    let max = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    assert!(max < 0.25, "max step {max}");
}

#[test]
fn lowering_polyphony_releases_extras() {
    let mut vm = VoiceManager::new(SR, 8);
    for n in [60, 64, 67, 71] {
        vm.note_on(n, 0.8);
    }
    render(&mut vm, 256, &default_values(), None);
    vm.set_polyphony(2);
    assert_eq!(sorted(vm.active_notes()), [67, 71]);
}

#[test]
fn polyphony_is_clamped() {
    let mut vm = VoiceManager::new(SR, 8);
    vm.set_polyphony(0);
    assert_eq!(vm.polyphony, 1);
    vm.set_polyphony(99);
    assert_eq!(vm.polyphony, 8);
    vm.set_polyphony(-5);
    assert_eq!(vm.polyphony, 1);
}

#[test]
fn glide_sanitised() {
    let mut vm = VoiceManager::new(SR, 8);
    for ms in [f64::NAN, -5.0, f64::INFINITY, 0.0] {
        vm.set_glide(120.0);
        vm.set_glide(ms);
        assert_eq!(vm.glide, 0.0, "{ms}");
    }
    vm.set_glide(80.0);
    assert_eq!(vm.glide, 80.0);
}

#[test]
fn panic_silences_everything() {
    let mut vm = VoiceManager::new(SR, 8);
    for n in [60, 64, 67, 71, 74] {
        vm.note_on(n, 1.0);
    }
    let p = default_values();
    render(&mut vm, 2048, &p, None);
    vm.panic();
    assert!(vm.active_notes().is_empty());
    render(&mut vm, 16 * 1000, &p, None);
    assert!(render(&mut vm, 1024, &p, None).iter().all(|&x| x == 0.0));
}

#[test]
fn fill_active_notes_matches_active_notes() {
    let mut vm = VoiceManager::new(SR, 4);
    for n in [60, 62, 64, 65, 67, 69] {
        vm.note_on(n, 0.8);
        render(&mut vm, 64, &default_values(), None);
    }
    let mut buf = [0i32; 128];
    let k = vm.fill_active_notes(&mut buf);
    assert_eq!(buf[..k].to_vec(), vm.active_notes());
    assert_eq!(sorted(vm.active_notes()), [64, 65, 67, 69]);
}
