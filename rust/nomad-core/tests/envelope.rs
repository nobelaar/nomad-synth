mod common;

use common::{Blob, assert_close, num};
use nomad_core::envelope::Envelope;

const SR: f64 = 48000.0;

/// Replay a command list from the reference meta; returns the concatenated runs.
fn replay(env: &mut Envelope, commands: &serde_json::Value) -> Vec<f64> {
    let mut all = Vec::new();
    for cmd in commands.as_array().unwrap() {
        let c = cmd.as_array().unwrap();
        match c[0].as_str().unwrap() {
            "params" => env.set_params(num(&c[1]), num(&c[2]), num(&c[3]), num(&c[4])),
            "on" => env.note_on(),
            "off" => env.note_off(),
            "run" => {
                let mut buf = vec![0.0; c[1].as_u64().unwrap() as usize];
                env.process(&mut buf);
                all.extend(buf);
            }
            other => panic!("unknown command {other}"),
        }
    }
    all
}

#[test]
fn scenarios_match_python() {
    let mut count = 0;
    for name in ["envelope", "envelope_more"] {
        let blob = Blob::open(name);
        for label in blob.labels() {
            let m = blob.meta(&label);
            let mut env = Envelope::new(SR);
            let got = replay(&mut env, &m["commands"]);
            assert_close(&got, blob.get(&label), &label);
            assert_eq!(env.is_idle(), m["idle_end"].as_bool().unwrap(), "{label}");
            count += 1;
        }
    }
    assert_eq!(count, 6);
    assert!(
        Blob::open("envelope").meta("adsr/basic")["idle_end"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn params_every_block_does_not_restart_attack() {
    let blob = Blob::open("envelope_more");
    let mut env = Envelope::new(SR);
    let got = replay(&mut env, &blob.meta("adsr/params_every_block")["commands"]);
    let peak = got
        .iter()
        .position(|&v| v == 1.0)
        .expect("attack reaches 1.0");
    assert!(got[..=peak].windows(2).all(|w| w[1] > w[0]));
}

#[test]
fn is_idle_before_note_on_and_after_release() {
    let mut env = Envelope::new(SR);
    assert!(env.is_idle());
    env.set_params(0.001, 0.01, 0.5, 0.01);
    env.note_on();
    assert!(!env.is_idle());
    env.process(&mut [0.0; 2000]);
    env.note_off();
    env.process(&mut [0.0; 4000]);
    assert!(env.is_idle());
    assert_eq!(env.value, 0.0);
}

#[test]
fn output_stays_within_0_and_1() {
    let mut env = Envelope::new(SR);
    let mut buf = [0.0; 256];
    for k in 0..200 {
        if k % 37 == 0 {
            env.note_on();
        }
        if k % 37 == 20 {
            env.note_off();
        }
        env.process(&mut buf);
        assert!(buf.iter().all(|&v| (0.0..=1.0).contains(&v)));
    }
}
