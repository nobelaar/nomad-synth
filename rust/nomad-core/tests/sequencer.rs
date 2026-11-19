mod common;

use common::{Blob, num};
use nomad_core::clock::Clock;
use nomad_core::sequencer::{NONE, NoteSink, StepSequencer};

const SR: f64 = 48000.0;
const ID: u64 = 1;

/// NoteSink that logs `[frame, on/off, note, velocity*127]` like the Python exporter.
#[derive(Default)]
struct Recorder {
    now: usize,
    trace: Vec<f64>,
}

impl NoteSink for Recorder {
    fn note_on(&mut self, note: i64, velocity: f64) {
        self.trace.extend([
            self.now as f64,
            1.0,
            note as f64,
            (velocity * 127.0).round(),
        ]);
    }
    fn note_off(&mut self, note: i64) {
        self.trace.extend([self.now as f64, 0.0, note as f64, 0.0]);
    }
}

fn run(m: &serde_json::Value) -> Vec<f64> {
    let mut seq = StepSequencer::new(SR);
    let mut clock = Clock::new(SR);
    clock.bpm = num(&m["bpm"]);
    seq.grid.division = num(&m["division"]);
    seq.grid.length = m["length"].as_u64().unwrap() as usize;
    seq.grid.swing = num(&m["swing"]);
    seq.set_gate(num(&m["gate"]));
    seq.enabled = m["enabled"].as_bool().unwrap();
    for s in m["steps"].as_array().unwrap() {
        let s: Vec<i64> = s
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_i64().unwrap())
            .collect();
        seq.set_step(s[0] as usize, s[1] != 0, Some(s[2]), Some(s[3]));
    }
    for e in m["extra"].as_array().unwrap() {
        seq.add_note(e[0].as_u64().unwrap() as usize, e[1].as_i64().unwrap());
    }
    let events = m["events"].as_array().unwrap();
    let total = m["frames"].as_u64().unwrap() as usize;
    let (mut sink, mut live) = (Recorder::default(), [false; 128]);
    let (mut i, mut ev) = (0usize, 0usize);
    let frame_of = |e: &serde_json::Value| e[0].as_u64().unwrap() as usize;
    while i < total {
        sink.now = i;
        while ev < events.len() && frame_of(&events[ev]) == i {
            let e = events[ev].as_array().unwrap();
            match e[1].as_str().unwrap() {
                "start" => clock.start(),
                "stop" => clock.stop(),
                "enable" => seq.enabled = e[2].as_bool().unwrap(),
                "live_on" => {
                    let n = e[2].as_i64().unwrap();
                    sink.note_on(n, num(&e[3]));
                    live[n as usize] = true;
                }
                "live_off" => {
                    let n = e[2].as_i64().unwrap();
                    sink.note_off(n);
                    live[n as usize] = false;
                }
                "transpose" => {
                    seq.transpose(e[2].as_u64().unwrap() as usize, e[3].as_i64().unwrap())
                }
                "clear" => seq.clear(),
                other => panic!("unknown event {other}"),
            }
            ev += 1;
        }
        seq.poll(&clock, ID, &mut sink, &live);
        let next = events.get(ev).map_or(total, frame_of);
        let chunk = (total - i)
            .min(seq.frames_to_event(&clock) as usize)
            .min(next - i);
        sink.now = i + chunk;
        seq.advance(chunk as i64, &mut sink, &live);
        clock.advance(chunk as i64);
        i += chunk;
    }
    seq.poll(&clock, ID, &mut sink, &live);
    sink.trace
}

#[test]
fn traces_match_python() {
    let blob = Blob::open("sequencer");
    let labels = blob.labels();
    assert_eq!(labels.len(), 10);
    for label in &labels {
        // Frames, notes and velocities are integers: exact comparison.
        assert_eq!(run(blob.meta(label)), blob.get(label), "{label}");
    }
}

#[test]
fn chord_api() {
    let mut seq = StepSequencer::new(SR);
    seq.set_step(3, true, Some(200), Some(0));
    assert_eq!((seq.note[3], seq.vel[3], seq.on[3]), (127, 1, 1));
    seq.set_step(2, true, Some(60), Some(100));
    for n in [64, 64, 60, 67, 70, 72, 74, 76] {
        seq.add_note(2, n);
    }
    assert_eq!(seq.chord(2), [60, 64, 67, 70, 72, 74]); // 76 is the 7th note: ignored
    seq.set_step(2, false, None, None);
    assert_eq!(seq.extra[2], [NONE; 5]);
    assert_eq!(seq.chord(2), [60]);
}

#[test]
fn transpose_clamps() {
    let mut seq = StepSequencer::new(SR);
    seq.set_step(0, true, Some(60), None);
    seq.add_note(0, 127);
    seq.transpose(0, 12);
    assert_eq!(seq.chord(0), [60, 127]);
    seq.set_step(1, true, Some(0), None);
    seq.add_note(1, 40);
    seq.transpose(1, -5);
    assert_eq!(seq.chord(1), [0, 40]);
    seq.transpose(1, 3);
    assert_eq!(seq.chord(1), [3, 43]);
}

#[test]
fn gate_sanitised() {
    let mut seq = StepSequencer::new(SR);
    assert_eq!(seq.gate(), 0.5);
    for (v, want) in [
        (f64::NAN, 0.5),
        (0.0, 0.05),
        (5.0, 1.0),
        (0.3, 0.3),
        (f64::INFINITY, 0.5),
    ] {
        seq.set_gate(v);
        assert_eq!(seq.gate(), want, "{v}");
    }
}

#[test]
fn position_follows_clock() {
    let seq = StepSequencer::new(SR);
    let mut clock = Clock::new(SR);
    clock.bpm = 120.0;
    assert_eq!(seq.position(&clock), 0);
    clock.start();
    clock.advance(6000 * 5 + 10);
    assert_eq!(seq.position(&clock), 5);
    clock.advance(6000 * 11);
    assert_eq!(seq.position(&clock), 0);
}

#[test]
fn stopped_clock_releases_hanging_note() {
    let mut seq = StepSequencer::new(SR);
    seq.enabled = true;
    seq.grid.division = 1.0;
    seq.set_step(0, true, Some(60), None);
    let mut clock = Clock::new(SR);
    clock.start();
    let (mut sink, live) = (Recorder::default(), [false; 128]);
    seq.poll(&clock, ID, &mut sink, &live);
    clock.advance(100);
    clock.stop();
    seq.poll(&clock, ID, &mut sink, &live);
    assert_eq!(sink.trace, [0.0, 1.0, 60.0, 100.0, 0.0, 0.0, 60.0, 0.0]);
    assert_eq!(seq.frames_to_event(&clock), nomad_core::clock::FAR);
}
