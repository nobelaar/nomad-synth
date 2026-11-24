//! MCP tools run on the UI thread against a real `NomadApp`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::Vec2;
use egui_kittest::Harness;
use nomad_app::mcp::tools;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::presets::PresetStore;
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::Engine;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, create};
use serde_json::{Value, json};

const SR: f64 = 48000.0;

struct S {
    app: NomadApp,
    core: AudioCore,
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nomad-mcp-tools-{tag}-{}-{:?}",
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

/// Run a tool on the app (as `Mcp::drain` would), then let the UI and core catch up.
#[allow(dead_code)]
fn call(h: &mut Harness<'static, S>, name: &str, args: Value) -> Result<Value, String> {
    let r = tools::call(&mut h.state_mut().app, &mut Vec::new(), name, &args);
    step(h);
    r
}

#[test]
fn unknown_tool_is_an_error() {
    let mut h = harness(&temp_dir("unknown"));
    assert_eq!(
        call(&mut h, "nope", json!({})),
        Err("unknown tool: nope".to_string())
    );
}
