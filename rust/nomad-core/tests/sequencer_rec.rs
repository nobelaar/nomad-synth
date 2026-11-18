mod common;

use common::Blob;
use nomad_core::clock::Clock;
use nomad_core::sequencer::StepSequencer;

const SR: f64 = 48000.0;

fn state(seq: &StepSequencer) -> Vec<f64> {
    let mut v: Vec<f64> = seq
        .on
        .iter()
        .chain(&seq.note)
        .chain(&seq.vel)
        .map(|&x| f64::from(x))
        .collect();
    v.extend(seq.extra.iter().flatten().map(|&x| f64::from(x)));
    v.push(seq.rec_step as f64);
    v
}

fn run_rec(commands: &serde_json::Value) -> Vec<f64> {
    let mut seq = StepSequencer::new(SR);
    let mut clock = Clock::new(SR);
    clock.bpm = 120.0;
    let (mut frame, mut out) = (0i64, Vec::new());
    for cmd in commands.as_array().unwrap() {
        let c = cmd.as_array().unwrap();
        let int = |k: usize| c[k].as_i64().unwrap();
        match c[0].as_str().unwrap() {
            "arm" => seq.arm(c[1].as_bool().unwrap()),
            "rec" => seq.record(int(1), int(2), &clock),
            "recoff" => seq.record_off(&clock),
            "rest" => seq.rest(&clock),
            "clock" => {
                if c[1] == "start" {
                    clock.start()
                } else {
                    clock.stop()
                }
                frame = 0;
            }
            "adv_to" => {
                clock.advance(int(1) - frame);
                frame = int(1);
            }
            other => panic!("unknown command {other}"),
        }
        out.extend(state(&seq));
    }
    out
}

#[test]
fn rec_scenarios_match_python() {
    let blob = Blob::open("sequencer_rec");
    assert_eq!(blob.labels().len(), 10);
    for label in blob.labels() {
        // Pattern state is integer data: exact comparison after every command.
        assert_eq!(
            run_rec(&blob.meta(&label)["commands"]),
            blob.get(&label),
            "{label}"
        );
    }
}

#[test]
fn arm_resets_cursor_and_counters() {
    let clock = Clock::new(SR);
    let mut seq = StepSequencer::new(SR);
    seq.arm(true);
    seq.record(60, 100, &clock);
    seq.record_off(&clock);
    seq.record(62, 100, &clock); // key still down
    assert_eq!(seq.rec_step, 1);
    seq.arm(true);
    assert!(seq.rec && seq.rec_step == 0);
    // The counter of keys down was cleared: the next key rewrites step 0.
    seq.record(70, 100, &clock);
    seq.record_off(&clock);
    assert_eq!((seq.chord(0), seq.rec_step), (vec![70], 1));
    seq.arm(false);
    assert!(!seq.rec && seq.rec_step == 0);
}

#[test]
fn rest_is_ignored_while_running_or_keys_down() {
    let mut clock = Clock::new(SR);
    let mut seq = StepSequencer::new(SR);
    seq.arm(true);
    seq.record(60, 100, &clock); // key down
    seq.rest(&clock);
    assert_eq!(seq.rec_step, 0);
    seq.record_off(&clock);
    assert_eq!(seq.rec_step, 1);
    clock.start();
    seq.rest(&clock);
    assert_eq!(seq.rec_step, 1);
    clock.stop();
    seq.rest(&clock);
    assert_eq!((seq.rec_step, seq.on[1]), (2, 0));
}
