use std::sync::{Arc, Mutex};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::{Engine, Kind};
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
}

fn harness(width: f32) -> Harness<'static, S> {
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
        .with_size(Vec2::new(width, 768.0))
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

fn step_rects(h: &Harness<'static, S>) -> Vec<egui::Rect> {
    h.query_all_by_label_contains("\n")
        .map(|n| n.rect())
        .collect()
}

#[test]
fn synth_tab_shows_sequencer_sections_keyboard_and_voice() {
    let h = harness(1366.0);
    for l in ["SEQ", "OSC1", "FILTER", "LFO2", "VOICE", "teclado"] {
        assert!(h.query_by_label(l).is_some(), "missing {l}");
    }
    assert!(h.query_by_label_contains("MASTER").is_some());
    assert_eq!(step_rects(&h).len(), 16);
}

#[test]
fn drum_tab_hides_keyboard_and_synth_panel() {
    let mut h = harness(1366.0);
    h.get_by_label("Machine Drum 01").click();
    h.run_steps(3);
    assert!(h.query_by_label("teclado").is_none());
    assert!(h.query_by_label("SEQ").is_none());
    assert!(h.query_by_label("KICK").is_some());
    assert!(h.query_by_label("snare 16").is_some());
}

#[test]
fn layout_at_640_1000_1366() {
    for w in [640.0, 1000.0, 1366.0] {
        let h = harness(w);
        for l in ["PLAY", "BPM", "teclado"] {
            assert!(h.query_by_label_contains(l).is_some(), "{w}: missing {l}");
        }
        assert!(
            h.query_by_label_contains("cutoff").is_some(),
            "{w}: cutoff knob"
        );
        let steps = step_rects(&h);
        assert_eq!(steps.len(), 16, "{w}");
        for (i, a) in steps.iter().enumerate() {
            assert!(a.max.x <= w + 0.5, "{w}: step {i} past the window ({a:?})");
            for b in &steps[i + 1..] {
                assert!(
                    !a.intersects(*b) || a.intersect(*b).area() < 1.0,
                    "{w}: steps overlap {a:?} {b:?}"
                );
            }
        }
    }
}

#[test]
fn header_wraps_without_clipping_status() {
    let mut h = harness(660.0);
    h.state_mut().app.status = "estado de prueba".into();
    h.run_steps(3);
    for l in ["XRUNS", "VOICES", "estado de prueba"] {
        let r = h.get_by_label_contains(l).rect();
        assert!(r.max.x <= 660.5 && r.min.x >= 0.0, "{l} clipped: {r:?}");
    }
}

#[test]
fn closing_a_tab_drops_its_panel_state() {
    let mut h = harness(1366.0);
    assert_eq!(h.state().app.panel_state_ids(), [1]);
    h.state_mut().app.model.close_instrument(1);
    h.run_steps(3);
    assert!(h.state().app.panel_state_ids().is_empty());
}

#[test]
fn switching_tabs_keeps_each_synth_state() {
    let mut h = harness(1366.0);
    h.get_by_label_contains("MODULACIÓN").click();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("PIT").count(), 8);
    let s2 = h.state_mut().app.model.add_instrument(Kind::Synth).unwrap();
    step(&mut h);
    assert_eq!(h.state().app.model.selected(), Some(s2));
    assert!(
        h.query_all_by_label("PIT").next().is_none(),
        "Synth 02's matrix is closed"
    );
    h.get_by_label("Synth 01").click();
    h.run_steps(3);
    assert_eq!(
        h.query_all_by_label("PIT").count(),
        8,
        "Synth 01 kept its open matrix"
    );
}

/// Renders the window at fixed widths into PNGs (needs a GPU adapter; run by hand:
/// `NOMAD_SHOTS=/some/dir cargo test -p nomad-app --features shots --test ui_synth shots -- --ignored`).
#[cfg(feature = "shots")]
#[test]
#[ignore = "renders with wgpu; run by hand for screenshots"]
fn shots() {
    let dir =
        std::env::var("NOMAD_SHOTS").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
    for w in [660.0, 1000.0, 1366.0] {
        let mut h = harness(w);
        h.state_mut()
            .app
            .model
            .send(nomad_runtime::Command::NoteOn {
                note: 48,
                velocity: 0.8,
            });
        h.state_mut()
            .app
            .model
            .send(nomad_runtime::Command::NoteOn {
                note: 55,
                velocity: 0.8,
            });
        h.state_mut()
            .app
            .model
            .send(nomad_runtime::Command::SeqSetStep {
                id: 1,
                step: 0,
                on: true,
                note: Some(48),
                vel: Some(127),
            });
        h.state_mut()
            .app
            .model
            .send(nomad_runtime::Command::SeqAddNote {
                id: 1,
                step: 0,
                note: 52,
            });
        h.state_mut()
            .app
            .model
            .send(nomad_runtime::Command::SeqSetStep {
                id: 1,
                step: 4,
                on: true,
                note: Some(55),
                vel: None,
            });
        for _ in 0..20 {
            step(&mut h);
        }
        let img = h.render().expect("render");
        let path = format!("{dir}/nomad-synth-{}.png", w as u32);
        img.save(&path).unwrap();
        println!("saved {path}");
    }
}
