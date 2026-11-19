//! Help mode: texts of `nomad/gui/help.py`, controls blocked, delayed text, F1/Esc.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::help::{HELP_DELAY_MS, HelpState, STATUS, help_for, registered};
use nomad_app::model::{AppModel, initial_views};
use nomad_app::synth_layout::short_label;
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::Engine;
use nomad_core::params::{DRUM_PARAMS, SYNTH_PARAMS, pid};
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
}

fn harness() -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let tel = ctl.telemetry();
    let model = AppModel::new(ctl, SR, views);
    let app = NomadApp::new(
        model,
        None,
        None,
        Arc::new(Mutex::new(MidiMap::new(None))),
        tel,
    );
    let mut h = Harness::builder()
        .with_size(Vec2::new(1366.0, 1400.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                s.app.ui(ui);
            },
            S { app, core },
        );
    step(&mut h);
    h
}

fn step(h: &mut Harness<'static, S>) {
    let mut out = vec![0.0f32; 512];
    h.state_mut().core.render_stereo(&mut out, 1);
    h.run_steps(3);
}

fn reference() -> serde_json::Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/help.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn knob_center(h: &Harness<'static, S>, p: usize) -> egui::Pos2 {
    h.query_all_by_label_contains(&format!("{} ", short_label(p)))
        .map(|n| n.rect())
        .find(|r| (r.width() - nomad_app::widgets::knob::W).abs() < 1.0)
        .expect("knob")
        .center()
}

fn keys(h: &Harness<'static, S>) -> BTreeSet<String> {
    registered(&h.ctx).into_iter().map(|(_, k)| k).collect()
}

fn press(h: &mut Harness<'static, S>, key: egui::Key) {
    h.key_press(key);
    h.run_steps(2);
}

#[test]
fn texts_match_python() {
    let r = reference();
    let texts = r["texts"].as_object().unwrap();
    assert!(texts.len() > 100);
    for (k, v) in texts {
        let (t, b) = help_for(k).unwrap_or_else(|| panic!("no help for {k}"));
        assert_eq!(
            (t, b),
            (v[0].as_str().unwrap(), v[1].as_str().unwrap()),
            "{k}"
        );
    }
    assert_eq!(help_for("filt_cutoff").unwrap().0, "Cutoff del filtro");
    assert_eq!(help_for("osc1_sync"), None, "not shown in Python either");
    assert_eq!(help_for("nope"), None);
}

#[test]
fn every_visible_control_has_help() {
    let mut h = harness();
    h.get_by_label_contains("MODULACIÓN").click();
    h.run_steps(3);
    let synth = keys(&h);
    for k in &synth {
        assert!(help_for(k).is_some(), "registered key without text: {k}");
    }
    for s in SYNTH_PARAMS.iter().filter(|s| s.name != "osc1_sync") {
        assert!(
            synth.contains(s.name),
            "synth control without help: {}",
            s.name
        );
    }
    for k in [
        "tab_select",
        "tab_close",
        "tab_new",
        "audio",
        "midi",
        "voices",
        "xruns",
        "learn",
        "help",
        "inst_play",
        "inst_bpm",
        "clock_source",
        "preset_menu",
        "preset_save",
        "inst_mute",
        "inst_level",
        "seq_toggle",
        "seq_rec",
        "seq_rest",
        "seq_div",
        "seq_len",
        "seq_gate",
        "seq_steps",
        "keyboard",
        "scope",
        "synth_view",
    ] {
        assert!(synth.contains(k), "missing {k}");
    }
    h.get_by_label("Machine Drum 01").click();
    h.run_steps(3);
    let drums = keys(&h);
    for k in &drums {
        assert!(help_for(k).is_some(), "registered key without text: {k}");
    }
    for s in DRUM_PARAMS.iter().filter(|s| s.name != "bpm") {
        assert!(
            drums.contains(s.name),
            "drum control without help: {}",
            s.name
        );
    }
    for k in [
        "drum_kick",
        "drum_ohat",
        "drum_steps",
        "drum_transport",
        "drums_view",
    ] {
        assert!(drums.contains(k), "missing {k}");
    }
}

#[test]
fn help_mode_blocks_controls() {
    let mut h = harness();
    h.get_by_label("?").click();
    h.run_steps(2);
    assert!(h.state().app.help.on);
    assert_eq!(h.state().app.status, STATUS);
    let before = h
        .state()
        .app
        .model
        .view(1)
        .unwrap()
        .shared
        .param(pid::FILT_CUTOFF);
    let c = knob_center(&h, pid::FILT_CUTOFF);
    h.event(Event::PointerMoved(c));
    h.step();
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for k in 1..=10 {
        h.event(Event::PointerMoved(c - Vec2::new(0.0, 5.0 * k as f32)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    h.get_by_label("SEQ").click();
    h.run_steps(2);
    step(&mut h);
    let v = h.state().app.model.view(1).unwrap().shared.clone();
    assert_eq!(
        v.param(pid::FILT_CUTOFF),
        before,
        "no SetParam in help mode"
    );
    assert!(!v.seq_enabled.load(std::sync::atomic::Ordering::Relaxed));
    // The ? button still works and turns it off.
    h.get_by_label("?").click();
    h.run_steps(2);
    assert!(!h.state().app.help.on);
    assert_eq!(h.state().app.status, "");
}

#[test]
fn tooltip_after_delay() {
    let t0 = Instant::now();
    let mut s = HelpState::default();
    s.toggle();
    s.hover("filt_cutoff", t0);
    assert_eq!(
        s.visible(t0 + Duration::from_millis(HELP_DELAY_MS - 10)),
        None
    );
    assert_eq!(
        s.visible(t0 + Duration::from_millis(HELP_DELAY_MS)),
        Some("filt_cutoff")
    );
    // Moving to another control keeps the old text until the new one is due.
    s.hover("filt_reso", t0 + Duration::from_millis(200));
    assert_eq!(
        s.visible(t0 + Duration::from_millis(250)),
        Some("filt_cutoff")
    );
    assert_eq!(
        s.visible(t0 + Duration::from_millis(400)),
        Some("filt_reso")
    );

    let mut h = harness();
    press(&mut h, egui::Key::F1);
    h.event(Event::PointerMoved(knob_center(&h, pid::FILT_CUTOFF)));
    h.run_steps(2);
    assert!(
        h.query_by_label("Cutoff del filtro").is_none(),
        "not before the delay"
    );
    std::thread::sleep(Duration::from_millis(HELP_DELAY_MS + 50));
    h.run_steps(2);
    assert!(h.query_by_label("Cutoff del filtro").is_some());
}

#[test]
fn f1_and_esc() {
    let mut h = harness();
    press(&mut h, egui::Key::Escape);
    assert!(!h.state().app.help.on, "Esc only turns it off");
    press(&mut h, egui::Key::F1);
    assert!(h.state().app.help.on);
    press(&mut h, egui::Key::F1);
    assert!(!h.state().app.help.on);
    press(&mut h, egui::Key::F1);
    press(&mut h, egui::Key::Escape);
    assert!(!h.state().app.help.on);
}
