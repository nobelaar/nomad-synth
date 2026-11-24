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
use nomad_core::engine::{Engine, Kind};
use nomad_core::params::{DRUM_PARAMS, SYNTH_PARAMS};
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

#[test]
fn get_state_lists_instruments() {
    let mut h = harness(&temp_dir("state"));
    let v = call(&mut h, "get_state", json!({})).unwrap();
    let app = &h.state().app;
    let insts = v["instruments"].as_array().unwrap();
    assert_eq!(insts.len(), app.model.instruments().len());
    for (j, i) in insts.iter().zip(app.model.instruments()) {
        assert_eq!(j["id"], json!(i.id));
        let k = if i.kind == Kind::Synth {
            "synth"
        } else {
            "drums"
        };
        assert_eq!(j["kind"], k);
        assert_eq!(j["name"], i.name.as_str());
    }
    assert_eq!(v["spiral_open"], false);
    assert!(v["master"].is_number());
    assert!(v["telemetry"]["peak"].is_number());
    assert!(v["telemetry"]["dropped_commands"].is_number());
    // null args behave like {}
    assert!(call(&mut h, "get_state", Value::Null).is_ok());
}

#[test]
fn get_instrument_captures_the_preset_shape() {
    let mut h = harness(&temp_dir("inst"));
    let id = h
        .state()
        .app
        .model
        .instruments()
        .iter()
        .find(|i| i.kind == Kind::Synth)
        .unwrap()
        .id;
    let v = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    let params = v["params"].as_object().unwrap();
    assert_eq!(params.len(), SYNTH_PARAMS.len() - 1);
    for s in SYNTH_PARAMS.iter().filter(|s| s.name != "master_volume") {
        assert!(params.contains_key(s.name), "missing {}", s.name);
    }
    assert_eq!(v["seq"]["on"].as_array().unwrap().len(), 16);
}

#[test]
fn get_instrument_errors() {
    let mut h = harness(&temp_dir("insterr"));
    assert_eq!(
        call(&mut h, "get_instrument", json!({ "id": 999 })),
        Err("no instrument with id 999".to_string())
    );
    let e = call(&mut h, "get_instrument", Value::Null).unwrap_err();
    assert!(e.contains("id"), "{e}");
}

#[test]
fn list_params_and_presets() {
    let mut h = harness(&temp_dir("lists"));
    let s = call(&mut h, "list_params", json!({ "kind": "synth" })).unwrap();
    assert_eq!(s.as_array().unwrap().len(), SYNTH_PARAMS.len());
    assert_eq!(s[0]["name"], SYNTH_PARAMS[0].name);
    for k in ["lo", "hi", "default", "unit"] {
        assert!(s[0].get(k).is_some(), "{k}");
    }
    let d = call(&mut h, "list_params", json!({ "kind": "drums" })).unwrap();
    assert_eq!(d.as_array().unwrap().len(), DRUM_PARAMS.len());
    let e = call(&mut h, "list_params", json!({ "kind": "x" })).unwrap_err();
    assert!(e.contains("synth") && e.contains("drums"), "{e}");
    assert!(call(&mut h, "list_params", json!({})).is_err());
    let p = call(&mut h, "list_presets", json!({ "kind": "synth" })).unwrap();
    assert!(p.as_array().unwrap().iter().any(|e| {
        e["origin"] == "factory" && e["name"].as_str().unwrap().contains("Dark techno")
    }));
    assert!(call(&mut h, "list_presets", json!({ "kind": "x" })).is_err());
}

#[test]
fn descriptors_cover_the_read_tools() {
    let d = tools::descriptors();
    for n in ["get_state", "get_instrument", "list_params", "list_presets"] {
        assert!(
            d.iter()
                .any(|t| t["name"] == n && t["inputSchema"]["type"] == "object"),
            "{n}"
        );
    }
}
