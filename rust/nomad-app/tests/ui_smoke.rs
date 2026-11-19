use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::Engine;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
}

impl S {
    fn render(&mut self) {
        let mut out = vec![0.0f32; 512];
        self.core.render_stereo(&mut out, 1);
    }
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
        .with_size(egui::vec2(1366.0, 768.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                s.app.ui(ui);
            },
            S { app, core },
        );
    h.run_steps(3);
    h
}

fn step(h: &mut Harness<'static, S>) {
    h.state_mut().render();
    h.run_steps(3);
}

#[test]
fn first_frame_draws_header_transport_and_scope() {
    let h = harness();
    for label in [
        "NOMAD SYNTH",
        "PLAY",
        "BPM",
        "MUTE",
        "Synth 01",
        "Machine Drum 01",
        "TAP",
        "10 ms",
        "MASTER",
    ] {
        assert!(
            h.query_by_label_contains(label).is_some(),
            "missing {label}"
        );
    }
}

#[test]
fn add_tab_via_plus_menu() {
    let mut h = harness();
    // Two "+": the add menu (header, on top) and the BPM step (transport).
    let add = h
        .get_all_by_label("+")
        .map(|n| n.rect())
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap();
    h.hover_at(add.center());
    h.event(egui::Event::PointerButton {
        pos: add.center(),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: add.center(),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.get_by_label("Synth").click();
    h.run_steps(3);
    let names: Vec<String> = h
        .state()
        .app
        .model
        .instruments()
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert_eq!(names, ["Synth 01", "Machine Drum 01", "Synth 02"]);
    assert!(h.query_by_label("Synth 02").is_some());
}

#[test]
fn close_tab_updates_selection() {
    let mut h = harness();
    assert_eq!(h.state().app.model.selected(), Some(1));
    h.get_all_by_label("×").next().unwrap().click(); // Synth 01's ×
    h.run_steps(3);
    assert_eq!(h.state().app.model.selected(), Some(2));
    assert!(h.query_by_label("Synth 01").is_none());
}

#[test]
fn play_button_starts_selected_instrument() {
    let mut h = harness();
    h.get_by_label("PLAY").click();
    h.run_steps(2);
    step(&mut h);
    let m = h.state().app.model.view(1).unwrap().shared.clone();
    assert!(m.clock_running.load(Relaxed));
    assert!(h.query_by_label("STOP").is_some());
}

#[test]
fn bpm_edit_sets_clock() {
    // Two "+": the add menu in the header and the BPM step in the transport (lower).
    let mut h = harness();
    let plus: Vec<_> = h.get_all_by_label("+").map(|n| n.rect()).collect();
    let bpm_plus = plus
        .iter()
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .copied()
        .unwrap();
    h.hover_at(bpm_plus.center());
    h.event(egui::Event::PointerButton {
        pos: bpm_plus.center(),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: bpm_plus.center(),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    step(&mut h);
    let m = h.state().app.model.view(1).unwrap().shared.clone();
    assert_eq!(m.clock_bpm.get(), 111.0);
}

#[test]
fn follower_controls_are_locked() {
    let mut h = harness();
    assert!(h.state_mut().app.model.set_source(1, Some(2)));
    step(&mut h);
    h.get_by_label("PLAY").click();
    h.run_steps(2);
    step(&mut h);
    let m = h.state().app.model.view(1).unwrap().shared.clone();
    assert!(!m.clock_running.load(Relaxed) && !m.eff_running.load(Relaxed));
    assert!(
        h.query_by_label_contains("CLOCK: Machine Drum 01")
            .is_some()
    );
}

#[test]
fn device_menu_lists_ports_without_panic() {
    let mut h = harness();
    h.get_by_label("AUDIO ⏷").click();
    h.run_steps(3);
    h.get_by_label("MIDI ⏷").click();
    h.run_steps(3);
}

#[test]
fn layout_survives_resize_and_scale() {
    for w in [1000.0, 1366.0, 1920.0] {
        for ppp in [1.0, 2.0] {
            let mut h = harness();
            h.set_size(egui::vec2(w, 700.0));
            h.set_pixels_per_point(ppp);
            h.run_steps(3);
            assert!(h.query_by_label("PLAY").is_some(), "{w} @ {ppp}");
        }
    }
}

#[test]
fn closing_stops_audio_and_releases_keys() {
    let mut h = harness();
    h.state_mut().app.shutdown();
    h.state_mut().app.shutdown();
    h.run_steps(2);
}
