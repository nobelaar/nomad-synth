//! Machine Drum view (`nomad/gui/drums_view.py`) with a hand-advanced `AudioCore`.

use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::drums_view::{DrumViewState, REGEN_COOLDOWN_S, drums_view_ui};
use nomad_app::model::{AppModel, initial_views};
use nomad_app::theme;
use nomad_core::engine::{Engine, InstrumentId};
use nomad_runtime::mirror::DrumSnapshot;
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

struct S {
    model: AppModel,
    core: AudioCore,
    state: DrumViewState,
    id: InstrumentId,
    now: Instant,
}

fn harness_for(id: InstrumentId, width: f32) -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let model = AppModel::new(ctl, SR, views);
    let mut h = Harness::builder()
        .with_size(Vec2::new(width, 600.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                let inst = s.model.view(s.id).cloned().unwrap();
                drums_view_ui(ui, &mut s.state, &inst, &mut s.model, s.now);
            },
            S {
                model,
                core,
                state: DrumViewState::default(),
                id,
                now: Instant::now(),
            },
        );
    step(&mut h);
    h
}

fn harness() -> Harness<'static, S> {
    harness_for(DRUMS, 1366.0)
}

fn render(h: &mut Harness<'static, S>) {
    let mut out = vec![0.0f32; 512];
    h.state_mut().core.render_stereo(&mut out, 1);
}

fn step(h: &mut Harness<'static, S>) {
    render(h);
    h.run_steps(3);
}

fn snap(h: &Harness<'static, S>) -> DrumSnapshot {
    let mut d = DrumSnapshot::default();
    h.state()
        .model
        .view(DRUMS)
        .unwrap()
        .shared
        .drum_snapshot(&mut d);
    d
}

fn click_at(h: &mut Harness<'static, S>, pos: egui::Pos2, mods: Modifiers) {
    h.event(Event::ModifiersChanged(mods));
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: mods,
        });
    }
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

fn wheel(h: &mut Harness<'static, S>, pos: egui::Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: Vec2::new(0.0, 1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.step();
}

#[test]
fn rows_and_controls_exist() {
    let h = harness();
    for l in ["KICK", "SNARE", "CLAP", "TOM", "CHAT", "OHAT", "CLEAR"] {
        assert!(h.query_by_label(l).is_some(), "missing {l}");
    }
    assert!(h.query_by_label_contains("SWING").is_some());
    for f in ["tune", "decay", "level"] {
        assert_eq!(h.query_all_by_label_contains(f).count(), 6, "{f}");
    }
    for t in ["kick", "ohat"] {
        for s in 1..=16 {
            assert!(h.query_by_label(&format!("{t} {s}")).is_some(), "{t} {s}");
        }
    }
}

#[test]
fn narrow_layout_puts_steps_under_knobs() {
    let h = harness_for(DRUMS, 800.0);
    let knob = h
        .query_all_by_label_contains("level")
        .next()
        .unwrap()
        .rect();
    let first = h.get_by_label("kick 1").rect();
    assert!(first.min.y > knob.max.y - 1.0, "steps go to their own line");
    let wide = harness();
    let knob = wide
        .query_all_by_label_contains("level")
        .next()
        .unwrap()
        .rect();
    let first = wide.get_by_label("kick 1").rect();
    assert!(first.min.x > knob.max.x, "one line when wide");
}

#[test]
fn click_and_shift_click_follow_python_rules() {
    let mut h = harness();
    let r = h.get_by_label("snare 3").rect();
    let level = |h: &mut Harness<'static, S>, mods| {
        click_at(h, r.center(), mods);
        step(h);
        snap(h).steps[1][2]
    };
    assert_eq!(level(&mut h, Modifiers::NONE), 1);
    assert_eq!(level(&mut h, Modifiers::NONE), 0);
    assert_eq!(level(&mut h, Modifiers::SHIFT), 2);
    assert_eq!(
        level(&mut h, Modifiers::NONE),
        0,
        "plain click turns an accent off"
    );
    assert_eq!(level(&mut h, Modifiers::SHIFT), 2);
    assert_eq!(level(&mut h, Modifiers::SHIFT), 0);
}

#[test]
fn name_button_triggers() {
    let mut h = harness();
    render(&mut h);
    let t = h.state().model.telemetry();
    assert_eq!(f32::from_bits(t.peak.load(Relaxed)), 0.0, "silent before");
    h.get_by_label("KICK").click();
    h.run_steps(2);
    render(&mut h);
    assert!(f32::from_bits(t.peak.load(Relaxed)) > 0.01);
}

#[test]
fn tune_change_regenerates_after_cooldown() {
    let mut h = harness();
    let t0 = h.state().now;
    let knob = h.query_all_by_label_contains("tune").next().unwrap().rect();
    // The first change regenerates at once (Python: nothing regenerated recently).
    wheel(&mut h, knob.center());
    render(&mut h);
    assert!(h.state_mut().model.tick() >= 1, "old kick shot came back");
    let first = snap(&h).values[0];
    assert!(first > 0.0);
    // A second change inside the cooldown is held back...
    h.state_mut().now = t0 + Duration::from_millis(10);
    wheel(&mut h, knob.center());
    render(&mut h);
    assert_eq!(h.state_mut().model.tick(), 0, "no DrumSetBank yet");
    assert!(snap(&h).values[0] > first, "the value itself went out");
    h.state_mut().now = t0 + Duration::from_millis(40);
    h.run_steps(2);
    render(&mut h);
    assert_eq!(h.state_mut().model.tick(), 0, "still inside the cooldown");
    // ...and sent once the cooldown is over.
    h.state_mut().now = t0 + Duration::from_secs_f64(REGEN_COOLDOWN_S + 0.005);
    h.run_steps(2);
    render(&mut h);
    assert!(h.state_mut().model.tick() >= 1);
}

#[test]
fn swing_and_clear() {
    let mut h = harness();
    let knob = h.get_by_label_contains("SWING").rect();
    wheel(&mut h, knob.center());
    step(&mut h);
    assert!(snap(&h).values[19] > 0.0);
    assert!(snap(&h).steps.iter().flatten().any(|&x| x > 0));
    h.get_by_label("CLEAR").click();
    h.run_steps(2);
    step(&mut h);
    assert!(snap(&h).steps.iter().flatten().all(|&x| x == 0));
}

#[test]
fn playhead_follows_mirror() {
    let mut h = harness();
    h.state_mut().model.send(Command::SetBpm {
        id: DRUMS,
        bpm: 120.0,
    });
    h.state_mut().model.send(Command::Start(DRUMS));
    for _ in 0..30 {
        step(&mut h);
    }
    let m = h.state().model.view(DRUMS).unwrap().shared.clone();
    assert!(m.step.load(Relaxed) >= 2);
    h.state_mut().model.send(Command::Stop(DRUMS));
    step(&mut h);
    assert_eq!(m.step.load(Relaxed), -1);
}

#[test]
fn follower_drum_shows_source_tempo() {
    let mut h = harness();
    assert!(h.state_mut().model.set_source(DRUMS, Some(SYNTH)));
    step(&mut h);
    assert!(
        h.query_by_label_contains("BPM").is_none(),
        "no tempo controls here"
    );
    assert!(h.query_by_label("KICK").is_some());
}
