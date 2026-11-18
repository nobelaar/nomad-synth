use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use nomad_app::model::{AppModel, initial_views};
use nomad_app::tap::TapTempo;
use nomad_core::engine::{Engine, InstrumentId, Kind, MAX_INSTANCES};
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

fn setup() -> (AudioCore, AppModel) {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    (core, AppModel::new(ctl, SR, views))
}

fn render(core: &mut AudioCore) {
    let mut out = vec![0.0f32; 512];
    core.render_stereo(&mut out, 1);
}

fn names(m: &AppModel) -> Vec<(InstrumentId, String)> {
    m.instruments()
        .iter()
        .map(|v| (v.id, v.name.clone()))
        .collect()
}

#[test]
fn add_names_and_ids() {
    let (mut core, mut m) = setup();
    assert_eq!(m.add_instrument(Kind::Synth), Some(3));
    assert_eq!(m.add_instrument(Kind::Drums), Some(4));
    assert_eq!(m.add_instrument(Kind::Synth), Some(5));
    let want: Vec<(u32, String)> = [
        (1, "Synth 01"),
        (2, "Machine Drum 01"),
        (3, "Synth 02"),
        (4, "Machine Drum 02"),
        (5, "Synth 03"),
    ]
    .iter()
    .map(|(i, n)| (*i, n.to_string()))
    .collect();
    assert_eq!(names(&m), want);
    assert_eq!(m.selected(), Some(5), "a new instrument is selected");
    render(&mut core);
    assert!(m.instruments().iter().all(|v| v.shared.id == v.id));
}

#[test]
fn add_respects_max_instances() {
    let (_core, mut m) = setup();
    while m.instruments().len() < MAX_INSTANCES {
        assert!(m.add_instrument(Kind::Drums).is_some());
    }
    assert_eq!(m.add_instrument(Kind::Synth), None);
}

#[test]
fn close_selected_picks_nearest_ready_tab() {
    let (_core, mut m) = setup();
    let a = m.add_instrument(Kind::Synth).unwrap(); // [1, 2, 3]
    m.add_instrument(Kind::Drums).unwrap(); // [1, 2, 3, 4]
    m.select(a);
    m.close_instrument(a);
    // Index 2 is gone: nearest by position are 2 (index 1) and 4 (now index 2): min distance, first wins.
    assert_eq!(m.selected(), Some(2));
    assert!(m.instruments().iter().all(|v| v.id != a));
}

#[test]
fn close_source_hands_followers_back_to_internal() {
    let (_core, mut m) = setup();
    assert!(m.set_source(SYNTH, Some(DRUMS)));
    assert_eq!(m.source_of(SYNTH), Some(DRUMS));
    m.close_instrument(DRUMS);
    assert_eq!(m.source_of(SYNTH), None);
}

#[test]
fn source_rules() {
    let (_core, mut m) = setup();
    let s2 = m.add_instrument(Kind::Synth).unwrap();
    assert!(m.set_source(SYNTH, Some(DRUMS)));
    assert!(!m.set_source(s2, Some(SYNTH)), "chain");
    assert!(!m.set_source(s2, Some(s2)), "self");
    assert!(
        !m.set_source(DRUMS, Some(s2)),
        "a source with followers cannot follow"
    );
    assert!(
        m.set_source(SYNTH, Some(DRUMS)),
        "same source again is fine"
    );
    m.close_instrument(s2);
    assert!(!m.set_source(SYNTH, Some(s2)), "closed source");
    assert!(!m.set_source(99, None));
}

#[test]
fn cycle_source_matches_python_order() {
    let (_core, mut m) = setup();
    let d2 = m.add_instrument(Kind::Drums).unwrap();
    assert_eq!(m.source_options(SYNTH), [None, Some(DRUMS), Some(d2)]);
    assert!(m.cycle_source(SYNTH));
    assert_eq!(m.source_of(SYNTH), Some(DRUMS));
    assert!(m.cycle_source(SYNTH));
    assert_eq!(m.source_of(SYNTH), Some(d2));
    assert!(m.cycle_source(SYNTH));
    assert_eq!(m.source_of(SYNTH), None);
    // A master (it has followers) can only be Internal.
    m.set_source(SYNTH, Some(DRUMS));
    assert_eq!(m.source_options(DRUMS), [None]);
}

#[test]
fn select_synth_sends_retarget_and_calls_hook() {
    let (mut core, mut m) = setup();
    let calls = Rc::new(Cell::new(0));
    let c = Rc::clone(&calls);
    m.set_on_retarget(Box::new(move || c.set(c.get() + 1)));
    let s2 = m.add_instrument(Kind::Synth).unwrap();
    assert_eq!(m.target_synth(), Some(s2));
    assert_eq!(calls.get(), 1);
    m.select(DRUMS);
    assert_eq!(
        m.target_synth(),
        Some(s2),
        "selecting drums keeps the target"
    );
    m.select(SYNTH);
    assert_eq!((m.target_synth(), calls.get()), (Some(SYNTH), 2));
    render(&mut core);
    assert_eq!(m.telemetry().target_synth.load(Relaxed), SYNTH);
}

#[test]
fn closing_last_synth_clears_target_without_panic() {
    let (mut core, mut m) = setup();
    m.close_instrument(SYNTH);
    assert_eq!(m.target_synth(), None);
    assert_eq!(m.selected(), Some(DRUMS));
    m.select(DRUMS);
    render(&mut core);
}

#[test]
fn deferred_drop_after_100ms() {
    let (mut core, mut m) = setup();
    let t0 = Instant::now();
    m.close_instrument_at(DRUMS, t0);
    render(&mut core);
    m.tick_at(t0 + Duration::from_millis(50));
    render(&mut core);
    assert_eq!(
        m.tick_at(t0 + Duration::from_millis(60)),
        0,
        "not dropped yet"
    );
    m.tick_at(t0 + Duration::from_millis(120)); // sends DropClosed
    render(&mut core);
    let reaped = m.tick_at(t0 + Duration::from_millis(130));
    assert_eq!(reaped, 1, "the closed machine came back as garbage");
}

#[test]
fn follower_play_is_noop() {
    let (mut core, mut m) = setup();
    m.set_source(SYNTH, Some(DRUMS));
    render(&mut core);
    assert!(!m.toggle_play(SYNTH));
    assert!(!m.set_bpm(SYNTH, 99.0));
    assert!(m.toggle_play(DRUMS));
    render(&mut core);
    let synth = m.view(SYNTH).unwrap().shared.clone();
    assert!(synth.eff_running.load(Relaxed) && !synth.clock_running.load(Relaxed));
}

#[test]
fn mute_intention_lives_in_the_model() {
    let (mut core, mut m) = setup();
    assert!(!m.muted(SYNTH));
    m.toggle_mute(SYNTH);
    assert!(m.muted(SYNTH));
    render(&mut core);
    assert!(m.view(SYNTH).unwrap().shared.muted.load(Relaxed));
}

// ---- TAP: InstanceStrip.tap ----

fn taps(intervals_s: &[f64]) -> Vec<Option<f64>> {
    let mut t = TapTempo::new();
    let t0 = Instant::now();
    let mut now = t0;
    let mut out = vec![t.tap(now)];
    for &dt in intervals_s {
        now += Duration::from_secs_f64(dt);
        out.push(t.tap(now));
    }
    out
}

#[test]
fn tap_averages_last_four_intervals() {
    // 0.5 s apart = 120 bpm; then the average of the last four intervals.
    assert_eq!(taps(&[0.5, 0.5]), [None, Some(120.0), Some(120.0)]);
    let r = taps(&[0.4, 0.4, 0.4, 0.4, 0.6]);
    // Last 5 taps span 0.4*3 + 0.6 = 1.8 s for 4 intervals: round(60*4/1.8) = 133.
    assert_eq!(r.last().copied().flatten(), Some(133.0));
}

#[test]
fn tap_resets_after_two_seconds() {
    let r = taps(&[0.5, 2.5, 0.25]);
    assert_eq!(r, [None, Some(120.0), None, Some(240.0)]);
}

#[test]
fn tap_clamps_to_20_300() {
    assert_eq!(taps(&[0.1]), [None, Some(300.0)]);
    assert_eq!(taps(&[1.99]), [None, Some(30.0)]);
    // Python's round() is half-to-even: 60/0.48 = 125.0 exactly is fine; 60*2/0.96... keep integral.
    assert_eq!(taps(&[0.48]).last().copied().flatten(), Some(125.0));
}
