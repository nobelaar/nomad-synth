mod common;

use common::{Blob, assert_engine_run, run_engine_ops};
use nomad_core::engine::{Engine, Kind};

const SR: f64 = 48000.0;
const SYNTH: u32 = 1;
const DRUMS: u32 = 2;

fn render(e: &mut Engine, n: usize) -> Vec<f64> {
    let mut o = vec![0.0; n];
    for part in o.chunks_mut(4096) {
        e.render(part);
    }
    o
}

#[test]
fn scenarios_match_python() {
    let mut count = 0;
    for name in ["topology", "topology_b", "topology_c"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            let mut e = Engine::new(SR, 8);
            let r = run_engine_ops(&mut e, &blob.meta(&label)["commands"]);
            assert_engine_run(&r, &blob, &label);
            count += 1;
        }
    }
    assert_eq!(count, 8);
}

#[test]
fn chain_rejected() {
    let mut e = Engine::new(SR, 8);
    let s2 = e.add_instrument(Kind::Synth);
    assert!(e.set_source(SYNTH, Some(DRUMS)));
    assert!(!e.set_source(s2, Some(SYNTH))); // SYNTH is a follower
    assert!(!e.set_source(SYNTH, Some(SYNTH)));
    assert!(!e.set_source(SYNTH, Some(99)));
    assert_eq!(e.source_of(SYNTH), Some(DRUMS));
    assert_eq!(e.followers_of(DRUMS), [SYNTH]);
}

#[test]
fn follower_cannot_start_or_set_bpm() {
    let mut e = Engine::new(SR, 8);
    e.set_source(SYNTH, Some(DRUMS));
    assert!(!e.start(SYNTH) && !e.stop(SYNTH) && !e.set_bpm(SYNTH, 100.0));
    assert!(e.start(DRUMS));
    assert!(e.effective_clock(SYNTH).unwrap().running);
    assert!(!e.clock(SYNTH).unwrap().running);
}

#[test]
fn source_with_followers_cannot_follow() {
    let mut e = Engine::new(SR, 8);
    let d2 = e.add_instrument(Kind::Drums);
    assert!(e.set_source(SYNTH, Some(DRUMS)));
    assert!(!e.set_source(DRUMS, Some(d2)));
    assert_eq!(e.source_of(DRUMS), None);
}

#[test]
fn remove_hands_over_before_silencing() {
    let mut e = Engine::new(SR, 8);
    e.set_source(SYNTH, Some(DRUMS));
    e.set_bpm(DRUMS, 133.0);
    e.start(DRUMS);
    render(&mut e, 10000);
    let master = e.clock(DRUMS).unwrap().clone();
    assert!(e.remove_instrument(DRUMS));
    let own = e.clock(SYNTH).unwrap();
    assert_eq!(
        (own.pos, own.bpm, own.running),
        (master.pos, master.bpm, true)
    );
    assert_eq!(e.source_of(SYNTH), None);
    // The closed one was silenced after the handover: its own clock is stopped.
    assert!(!e.clock(DRUMS).unwrap().running);
    assert!(!e.remove_instrument(DRUMS));
}

#[test]
fn closing_instance_renders_until_dropped() {
    let mut e = Engine::new(SR, 8);
    e.note_on(60, 1.0);
    render(&mut e, 4096);
    e.remove_instrument(SYNTH);
    assert!(e.instruments().iter().all(|(id, ..)| *id != SYNTH));
    let fade = render(&mut e, 256);
    assert!(fade[..8].iter().any(|&x| x != 0.0), "still fading out");
    render(&mut e, 2048);
    e.drop_closed();
    assert!(e.clock(SYNTH).is_none());
    assert!(render(&mut e, 1024).iter().all(|&x| x == 0.0));
}
