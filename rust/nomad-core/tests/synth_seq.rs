mod common;

use common::{Blob, assert_close, num, run_single_instrument};
use nomad_core::clock::Clock;
use nomad_core::params::SYNTH_PARAMS;
use nomad_core::synth::Synth;

const SR: f64 = 48000.0;
const ID: u64 = 1;

fn id(name: &str) -> usize {
    SYNTH_PARAMS.iter().position(|s| s.name == name).unwrap()
}

fn u8s(v: &serde_json::Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect()
}

fn run(m: &serde_json::Value) -> Vec<f64> {
    let mut s = Synth::new(SR, 8);
    let mut clock = Clock::new(SR);
    for pv in m["params"].as_array().unwrap() {
        s.set_param(id(pv[0].as_str().unwrap()), num(&pv[1]));
    }
    let seq = &m["seq"];
    let sq = &mut s.sequencer;
    sq.on.copy_from_slice(&u8s(&seq["on"]));
    sq.note.copy_from_slice(&u8s(&seq["note"]));
    sq.vel.copy_from_slice(&u8s(&seq["vel"]));
    for (row, e) in sq.extra.iter_mut().zip(seq["extra"].as_array().unwrap()) {
        row.copy_from_slice(&u8s(e));
    }
    sq.set_gate(num(&seq["gate"]));
    sq.grid.division = num(&seq["division"]);
    sq.grid.length = seq["length"].as_u64().unwrap() as usize;
    sq.grid.swing = num(&seq["swing"]);
    clock.bpm = num(&m["bpm"]);
    let (n_cb, cb_len) = (
        m["callbacks"].as_u64().unwrap() as usize,
        m["callback"].as_u64().unwrap() as usize,
    );
    let events = m["events"].as_array().unwrap();
    let mut out = vec![0.0; n_cb * cb_len];
    for (cb, buf) in out.chunks_mut(cb_len).enumerate() {
        for e in events
            .iter()
            .filter(|e| e[0].as_u64().unwrap() as usize == cb)
        {
            let e = e.as_array().unwrap();
            match e[1].as_str().unwrap() {
                "on" => s.note_on(e[2].as_i64().unwrap(), num(&e[3]), &clock),
                "off" => s.note_off(e[2].as_i64().unwrap(), &clock),
                "arm" => s.arm_rec(e[2].as_bool().unwrap()),
                "enable" => s.sequencer.enabled = e[2].as_bool().unwrap(),
                "length" => s.sequencer.grid.length = e[2].as_u64().unwrap() as usize,
                "division" => s.sequencer.grid.division = num(&e[2]),
                "start" => clock.start(),
                other => panic!("unknown event {other}"),
            }
        }
        run_single_instrument(&mut s, &mut clock, ID, buf);
    }
    let decim = m["decim"].as_u64().unwrap() as usize;
    out.into_iter().step_by(decim).collect()
}

#[test]
fn audio_scenarios_match_python() {
    let mut count = 0;
    for name in ["synth_seq", "synth_seq_more"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            // Decimated reference; any divergence propagates through the voices.
            assert_close(&run(blob.meta(&label)), blob.get(&label), &label);
            count += 1;
        }
    }
    assert_eq!(count, 4);
}

#[test]
fn rec_through_synth_note_on_writes_pattern() {
    let clock = Clock::new(SR);
    let mut s = Synth::new(SR, 8);
    s.arm_rec(true);
    for n in [60, 64, 67] {
        s.note_on(n, 0.8, &clock);
    }
    for n in [60, 64, 67] {
        s.note_off(n, &clock);
    }
    assert_eq!(s.sequencer.chord(0), [60, 64, 67]);
    assert_eq!((s.sequencer.vel[0], s.sequencer.rec_step), (101, 1)); // int(0.8 * 127)
}

#[test]
fn panic_disarms_and_releases() {
    let mut clock = Clock::new(SR);
    let mut s = Synth::new(SR, 8);
    s.sequencer.enabled = true;
    s.sequencer.set_step(0, true, Some(60), None);
    s.arm_rec(true);
    clock.start();
    let mut buf = vec![0.0; 512];
    run_single_instrument(&mut s, &mut clock, ID, &mut buf);
    s.note_on(72, 1.0, &clock);
    s.panic();
    assert!(!s.sequencer.rec && !s.is_live(72));
    clock.stop();
    for _ in 0..4 {
        run_single_instrument(&mut s, &mut clock, ID, &mut buf);
    }
    assert!(buf.iter().all(|&x| x == 0.0));
}

#[test]
fn live_key_is_not_cut_by_sequencer() {
    // Step 0 plays 64; its gate ends at 3000 frames. Without the live key the voice
    // releases (400 ms) and is silent by 1.6 s; with it, the key keeps sustaining.
    let play = |live: bool| {
        let mut clock = Clock::new(SR);
        clock.bpm = 120.0;
        let mut s = Synth::new(SR, 8);
        s.sequencer.enabled = true;
        s.sequencer.set_step(0, true, Some(64), None);
        if live {
            s.note_on(64, 0.9, &clock);
        }
        clock.start();
        let mut buf = vec![0.0; 512];
        for _ in 0..150 {
            run_single_instrument(&mut s, &mut clock, ID, &mut buf);
        }
        assert_eq!(s.is_live(64), live);
        buf.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
    };
    assert_eq!(play(false), 0.0);
    assert!(play(true) > 0.05);
}
