//! MIDI learn from the UI: context menu, LEARN button, CC through `MidiHandler::feed`,
//! the map saved to a temporary file.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::learn::LearnState;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::synth_layout::short_label;
use nomad_app::theme::{self, GREEN, GREY};
use nomad_app::ui::NomadApp;
use nomad_app::widgets::knob::knob_colors;
use nomad_core::engine::Engine;
use nomad_core::params::pid;
use nomad_runtime::midi::input::MidiHandler;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
    midi: MidiHandler,
}

fn map_path(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nomad-learn-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("midi-map.json")
}

fn harness(path: &Path) -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, mut ctl) = create(e, SR);
    let sender = ctl.midi_sender().unwrap();
    let tel = ctl.telemetry();
    let map = Arc::new(Mutex::new(MidiMap::new(Some(path.to_path_buf()))));
    let midi = MidiHandler::new(sender, Arc::clone(&map), "TEST:TEST MIDI 1 24:0".into());
    let model = AppModel::new(ctl, SR, views);
    let app = NomadApp::new(model, None, None, map, tel);
    let mut h = Harness::builder()
        .with_size(Vec2::new(1366.0, 1000.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                s.app.ui(ui);
            },
            S { app, core, midi },
        );
    step(&mut h);
    h
}

fn step(h: &mut Harness<'static, S>) {
    let mut out = vec![0.0f32; 512];
    h.state_mut().core.render_stereo(&mut out, 1);
    h.run_steps(3);
}

/// The knob itself (64 px wide), not the status line that names it.
fn knob_rect(h: &Harness<'static, S>, p: usize) -> egui::Rect {
    h.query_all_by_label_contains(&format!("{} ", short_label(p)))
        .map(|n| n.rect())
        .find(|r| (r.width() - nomad_app::widgets::knob::W).abs() < 1.0)
        .expect("knob")
}

fn right_click(h: &mut Harness<'static, S>, pos: egui::Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
}

fn menu(h: &mut Harness<'static, S>, p: usize, item: &str) {
    let r = knob_rect(h, p);
    right_click(h, r.center());
    h.get_by_label(item).click();
    h.run_steps(3);
}

fn status(h: &Harness<'static, S>) -> String {
    h.state().app.status.clone()
}

fn cc(h: &mut Harness<'static, S>, n: u8, v: u8) {
    h.state_mut().midi.feed(&[0xB0, n, v]);
    step(h);
}

#[test]
fn learn_without_touch_reports() {
    let mut h = harness(&map_path("notouch"));
    h.get_by_label("LEARN").click();
    h.run_steps(2);
    assert_eq!(status(&h), "LEARN: touch a control first");
}

#[test]
fn context_menu_learn_then_cc_binds_and_saves() {
    let path = map_path("bind");
    let mut h = harness(&path);
    menu(&mut h, pid::FILT_CUTOFF, "MIDI learn");
    assert_eq!(status(&h), "LEARN: filt_cutoff — move a CC");
    assert_eq!(h.state().app.learn.armed_by_us, Some(pid::FILT_CUTOFF));
    cc(&mut h, 21, 64);
    assert_eq!(status(&h), "LEARN: filt_cutoff bound");
    assert_eq!(
        h.state().app.learn.armed_by_us,
        None,
        "no longer highlighted"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"filt_cutoff\"") && text.contains("\"cc\": 21"),
        "{text}"
    );
    // The bound CC now moves the knob's parameter.
    cc(&mut h, 21, 127);
    let v = h
        .state()
        .app
        .model
        .view(1)
        .unwrap()
        .shared
        .param(pid::FILT_CUTOFF);
    assert_eq!(v, nomad_core::params::SYNTH_PARAMS[pid::FILT_CUTOFF].hi);
}

#[test]
fn learn_button_uses_last_touched() {
    let mut h = harness(&map_path("last"));
    let r = knob_rect(&h, pid::FILT_RESO);
    h.event(Event::PointerMoved(r.center()));
    h.step();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: Vec2::new(0.0, 1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(2);
    h.get_by_label("LEARN").click();
    h.run_steps(2);
    assert_eq!(status(&h), "LEARN: filt_reso — move a CC");
    // LEARN again cancels.
    h.get_by_label("LEARN").click();
    h.run_steps(2);
    assert_eq!(status(&h), "");
    assert!(h.state().app.midi_map.lock().unwrap().learning().is_none());
}

#[test]
fn cancel_learn() {
    let mut h = harness(&map_path("cancel"));
    menu(&mut h, pid::FILT_CUTOFF, "MIDI learn");
    menu(&mut h, pid::FILT_CUTOFF, "Cancelar learn");
    assert_eq!(status(&h), "");
    assert_eq!(h.state().app.learn.armed_by_us, None);
    assert!(h.state().app.midi_map.lock().unwrap().learning().is_none());
    cc(&mut h, 21, 64);
    assert!(h.state().app.midi_map.lock().unwrap().bindings().is_empty());
}

#[test]
fn unbind_removes_and_saves() {
    let path = map_path("unbind");
    let mut h = harness(&path);
    let r = knob_rect(&h, pid::FILT_CUTOFF);
    right_click(&mut h, r.center());
    assert!(
        h.query_by_label("Quitar MIDI").is_none(),
        "nothing bound yet"
    );
    h.get_by_label("MIDI learn").click();
    h.run_steps(3);
    cc(&mut h, 21, 64);
    menu(&mut h, pid::FILT_CUTOFF, "Quitar MIDI");
    assert_eq!(status(&h), "MIDI: filt_cutoff desvinculado");
    assert!(h.state().app.midi_map.lock().unwrap().bindings().is_empty());
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("filt_cutoff")
    );
}

#[test]
fn armed_knob_is_highlighted() {
    assert_eq!(knob_colors(false, true), (GREEN, GREEN));
    assert_eq!(knob_colors(false, false), (GREY, GREY));
    assert_eq!(knob_colors(true, false), (GREEN, GREY));
    let map = Mutex::new(MidiMap::new(None));
    let mut l = LearnState::default();
    l.arm(&map, Some(pid::GLIDE));
    assert_eq!(l.armed_by_us, Some(pid::GLIDE));
    assert_eq!(map.lock().unwrap().learning(), Some("glide"));
}

#[test]
fn drum_knobs_have_no_learn_entry() {
    let mut h = harness(&map_path("drums"));
    h.get_by_label("Machine Drum 01").click();
    h.run_steps(3);
    let r = h.query_all_by_label_contains("tune").next().unwrap().rect();
    right_click(&mut h, r.center());
    assert!(h.query_by_label("Valor por defecto").is_some());
    assert!(h.query_by_label("MIDI learn").is_none());
}
