mod common;

use common::{Blob, assert_close, num, nums};
use nomad_core::modmatrix::{Destinations, ModMatrix, Sources};

fn sources(v: &[f64]) -> Sources {
    Sources {
        lfo1: v[0],
        lfo2: v[1],
        modwheel: v[2],
        velocity: v[3],
        env2: v[4],
    }
}

fn flat(d: &Destinations) -> Vec<f64> {
    vec![d.pitch, d.pitch1, d.pitch2, d.amp, d.cutoff, d.pw1, d.pw2]
}

#[test]
fn scenarios_match_python() {
    let blob = Blob::open("modmatrix");
    assert_eq!(blob.labels().len(), 6);
    for label in blob.labels() {
        let m = blob.meta(&label);
        let mut mm = ModMatrix::new();
        for slot in m["slots"].as_array().unwrap() {
            let s = slot.as_array().unwrap();
            let idx = |k: usize| s[k].as_u64().unwrap() as usize;
            mm.set_slot(idx(0), idx(1), idx(2), num(&s[3])).unwrap();
        }
        let got = flat(mm.resolve(&sources(&nums(&m["sources"]))));
        assert_close(&got, blob.get(&label), &label);
    }
}

#[test]
fn invalid_slot_rejected_state_untouched() {
    let mut mm = ModMatrix::new();
    mm.set_slot(0, 1, 5, 0.5).unwrap();
    let src = sources(&[0.4, 0.0, 0.0, 0.0, 0.0]);
    let before = *mm.resolve(&src);
    assert!(mm.set_slot(8, 1, 1, 1.0).is_err());
    assert!(mm.set_slot(0, 6, 1, 1.0).is_err());
    assert!(mm.set_slot(0, 1, 8, 1.0).is_err());
    assert_eq!(*mm.resolve(&src), before);
    assert_eq!(before.cutoff, 0.2);
}

#[test]
fn resolve_resets_between_calls() {
    let mut mm = ModMatrix::new();
    mm.set_slot(2, 3, 4, 1.0).unwrap();
    let a = *mm.resolve(&sources(&[0.0, 0.0, 0.7, 0.0, 0.0]));
    let b = *mm.resolve(&sources(&[0.0, 0.0, 0.2, 0.0, 0.0]));
    assert_eq!((a.amp, b.amp), (0.7, 0.2));
}
