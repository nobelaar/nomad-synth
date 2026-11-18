mod common;

use common::{Blob, assert_close, num};
use nomad_core::clock::{Clock, EPS, FAR, StepGrid, StepTracker};

const SR: f64 = 48000.0;

fn run_track(commands: &serde_json::Value) -> Vec<f64> {
    let mut clocks = [Clock::new(SR), Clock::new(SR)];
    let mut grid = StepGrid::new(0.25, 16, 0.0);
    let mut tr = StepTracker::new();
    let mut cur = 0usize; // index into clocks; the id passed to the tracker is cur + 1
    let mut out = Vec::new();
    for cmd in commands.as_array().unwrap() {
        let c = cmd.as_array().unwrap();
        let idx = |k: usize| c[k].as_u64().unwrap() as usize - 1;
        match c[0].as_str().unwrap() {
            "start" => clocks[idx(1)].start(),
            "stop" => clocks[idx(1)].stop(),
            "bpm" => clocks[idx(1)].bpm = num(&c[2]),
            "adv" => clocks
                .iter_mut()
                .for_each(|k| k.advance(c[1].as_i64().unwrap())),
            "adv_fte" => {
                let n = tr.frames_to_event(&grid, &clocks[cur]);
                clocks.iter_mut().for_each(|k| k.advance(n));
            }
            "swing" => grid.swing = num(&c[1]),
            "div" => grid.division = num(&c[1]),
            "len" => grid.length = c[1].as_u64().unwrap() as usize,
            "clock" => cur = idx(1),
            "poll" => {
                let s = tr.poll(&grid, &clocks[cur], cur as u64 + 1);
                out.push(s.map_or(-1.0, |s| s as f64));
                out.push(tr.frames_to_event(&grid, &clocks[cur]) as f64);
            }
            other => panic!("unknown command {other}"),
        }
    }
    out
}

#[test]
fn grid_matches_python() {
    let blob = Blob::open("clock");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("grid/") && !l.ends_with("/pos") && !l.ends_with("/k"))
        .collect();
    assert_eq!(labels.len(), 6);
    for label in &labels {
        let m = blob.meta(label);
        let g = StepGrid::new(num(&m["division"]), 16, num(&m["swing"]));
        let want = blob.get(label);
        for (i, &p) in blob.get(&format!("{label}/pos")).iter().enumerate() {
            assert_eq!(g.step_at(p) as f64, want[2 * i], "{label} step_at({p})");
            assert_close(&[g.next_boundary(p)], &[want[2 * i + 1]], label);
        }
        let k = blob.get(&format!("{label}/k"));
        for i in 0..33 {
            assert_close(
                &[g.step_beats(i as i64), g.step_start(i as i64)],
                &k[2 * i..2 * i + 2],
                label,
            );
        }
    }
}

#[test]
fn tracker_scenarios_match_python() {
    let blob = Blob::open("clock");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("track/"))
        .collect();
    assert_eq!(labels.len(), 10);
    for label in &labels {
        // Steps and frames are integers: compared exactly.
        assert_eq!(
            run_track(&blob.meta(label)["commands"]),
            blob.get(label),
            "{label}"
        );
    }
}

#[test]
fn clock_basics() {
    let mut c = Clock::new(SR);
    assert_eq!(c.bpm, 110.0);
    assert_eq!(c.frames_to(1.0), FAR);
    c.bpm = 120.0;
    c.start();
    assert_eq!(c.frames_to(1.0), 24000);
    c.advance(1000);
    let pos = c.pos;
    c.start(); // already running: no restart
    assert_eq!(c.pos, pos);
    c.stop();
    assert_eq!((c.pos, c.running), (0.0, false));
    c.advance(1000);
    assert_eq!(c.pos, 0.0);
    c.toggle();
    assert!(c.running);
    c.toggle();
    assert!(!c.running);
}

#[test]
fn pattern_step_is_never_negative() {
    let g = StepGrid::new(0.25, 5, 0.0);
    assert_eq!(g.pattern_step(-1), 4);
    assert_eq!(g.pattern_step(-5), 0);
    assert_eq!(g.pattern_step(7), 2);
    assert_eq!(g.step_at(-EPS), 0);
    assert_eq!(g.step_at(-0.1), -1);
}

#[test]
fn no_repeated_step_on_swing_drag() {
    let blob = Blob::open("clock");
    let trace = blob.get("track/swing_drag");
    let fired: Vec<f64> = trace
        .chunks(2)
        .map(|p| p[0])
        .filter(|&s| s >= 0.0)
        .collect();
    assert!(fired.len() >= 3);
    assert!(fired.windows(2).all(|w| w[1] != w[0]), "{fired:?}");
}
