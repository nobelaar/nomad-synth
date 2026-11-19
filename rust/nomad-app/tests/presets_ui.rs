//! PRESET menu and SAVE dialog of the transport bar (`strip.py`), with a
//! `PresetStore` in a temporary directory.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Event, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::presets::{PresetKind, PresetStore, factory};
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::Engine;
use nomad_core::params::pid;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::mirror::DrumSnapshot;
use nomad_runtime::{AudioCore, create};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nomad-presets-ui-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn harness(dir: &Path) -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let tel = ctl.telemetry();
    let model = AppModel::new(ctl, SR, views);
    let mut app = NomadApp::new(
        model,
        None,
        None,
        Arc::new(Mutex::new(MidiMap::new(None))),
        tel,
    );
    app.presets = PresetStore::new(Some(dir.to_path_buf()));
    let mut h = Harness::builder()
        .with_size(Vec2::new(1366.0, 768.0))
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

fn open_preset_menu(h: &mut Harness<'static, S>, label: &str) {
    h.get_by_label(&format!("{label} ⏷")).click();
    h.run_steps(3);
}

fn save_as(h: &mut Harness<'static, S>, name: &str) {
    h.get_by_label("SAVE").click();
    h.run_steps(3);
    h.event(Event::Text(name.into()));
    h.run_steps(2);
    h.get_by_label("Guardar").click();
    h.run_steps(3);
}

#[test]
fn menu_lists_factory_then_user() {
    let dir = temp_dir("list");
    PresetStore::new(Some(dir.clone()))
        .save(PresetKind::Synth, "Mio", &factory(PresetKind::Synth)[2])
        .unwrap();
    let mut h = harness(&dir);
    open_preset_menu(&mut h, "PRESET");
    for l in ["Pad de acordes", "Bajo", "Lead", "Pluck", "Mio"] {
        assert!(h.query_by_label(l).is_some(), "missing {l}");
    }
    let y = |h: &Harness<'static, S>, l: &str| h.get_by_label(l).rect().min.y;
    assert!(y(&h, "Pluck") < y(&h, "Mio"), "factory first");
}

#[test]
fn selecting_factory_preset_applies_it() {
    let dir = temp_dir("apply");
    let mut h = harness(&dir);
    open_preset_menu(&mut h, "PRESET");
    h.get_by_label("Bajo").click();
    h.run_steps(2);
    step(&mut h);
    let v = h.state().app.model.view(1).unwrap().shared.clone();
    assert_eq!(v.param(pid::FILT_CUTOFF), 400.0);
    assert_eq!(v.param(pid::GLIDE), 60.0);
    assert!(h.query_by_label("Bajo ⏷").is_some());
}

#[test]
fn save_dialog_writes_file_and_lists_it() {
    let dir = temp_dir("save");
    let mut h = harness(&dir);
    save_as(&mut h, "Mi bajo!");
    assert!(dir.join("synth/Mi bajo.json").is_file());
    assert_eq!(h.state().app.status, "PRESET guardado: Mi bajo");
    assert!(h.query_by_label("Mi bajo ⏷").is_some());
    assert!(h.query_by_label("Guardar").is_none(), "dialog closed");
    open_preset_menu(&mut h, "Mi bajo");
    assert!(h.query_by_label("Mi bajo").is_some());
}

#[test]
fn save_reserved_name_shows_error() {
    let dir = temp_dir("reserved");
    let mut h = harness(&dir);
    save_as(&mut h, "bajo");
    assert_eq!(
        h.state().app.status,
        "PRESET: ese nombre es de un preset de fábrica"
    );
    assert!(!dir.join("synth").exists(), "nothing written");
    // Cancel leaves everything as it was.
    h.get_by_label("SAVE").click();
    h.run_steps(3);
    h.get_by_label("Cancelar").click();
    h.run_steps(3);
    assert!(h.query_by_label("Guardar").is_none());
}

#[test]
fn drum_preset_from_menu() {
    let dir = temp_dir("drums");
    let mut h = harness(&dir);
    h.get_by_label("Machine Drum 01").click();
    h.run_steps(3);
    open_preset_menu(&mut h, "PRESET");
    assert!(
        h.query_by_label("Pad de acordes").is_none(),
        "drum list only"
    );
    h.get_by_label("Breakbeat").click();
    h.run_steps(2);
    step(&mut h);
    let mut d = DrumSnapshot::default();
    h.state()
        .app
        .model
        .view(2)
        .unwrap()
        .shared
        .drum_snapshot(&mut d);
    assert_eq!(d.steps[1][4], 2);
    assert_eq!(d.values[4], 0.4);
    assert!(h.query_by_label("Breakbeat ⏷").is_some());
}

#[test]
fn invalid_user_file_is_not_listed() {
    let dir = temp_dir("invalid");
    std::fs::create_dir_all(dir.join("synth")).unwrap();
    std::fs::write(
        dir.join("synth/roto.json"),
        r#"{"kind": "synth", "name": "Roto""#,
    )
    .unwrap();
    let mut h = harness(&dir);
    open_preset_menu(&mut h, "PRESET");
    assert!(h.query_by_label("Bajo").is_some());
    assert!(h.query_by_label("Roto").is_none());
}
