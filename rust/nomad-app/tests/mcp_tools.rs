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

fn first_id(h: &Harness<'static, S>, kind: Kind) -> u32 {
    h.state()
        .app
        .model
        .instruments()
        .iter()
        .find(|i| i.kind == kind)
        .unwrap()
        .id
}

fn param(h: &mut Harness<'static, S>, id: u32, name: &str) -> f64 {
    call(h, "get_instrument", json!({ "id": id })).unwrap()["params"][name]
        .as_f64()
        .unwrap()
}

#[test]
fn add_and_close_instrument() {
    let mut h = harness(&temp_dir("add"));
    let n = h.state().app.model.instruments().len();
    let r = call(&mut h, "add_instrument", json!({ "kind": "synth" })).unwrap();
    let id = r["id"].as_u64().unwrap();
    let s = call(&mut h, "get_state", json!({})).unwrap();
    assert_eq!(s["instruments"].as_array().unwrap().len(), n + 1);
    assert!(
        s["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == id)
    );
    call(&mut h, "select_instrument", json!({ "id": id })).unwrap();
    call(&mut h, "set_target", json!({ "id": id })).unwrap();
    let s = call(&mut h, "get_state", json!({})).unwrap();
    assert_eq!(
        (s["selected"].clone(), s["target"].clone()),
        (json!(id), json!(id))
    );
    call(&mut h, "close_instrument", json!({ "id": id })).unwrap();
    assert_eq!(
        call(&mut h, "select_instrument", json!({ "id": id })),
        Err(format!("no instrument with id {id}"))
    );
    assert!(call(&mut h, "add_instrument", json!({ "kind": "x" })).is_err());
    let d = first_id(&h, Kind::Drums);
    let e = call(&mut h, "set_target", json!({ "id": d })).unwrap_err();
    assert!(e.contains("drums"), "{e}");
    let mut last = Ok(json!(null));
    for _ in 0..40 {
        last = call(&mut h, "add_instrument", json!({ "kind": "drums" }));
        if last.is_err() {
            break;
        }
    }
    assert_eq!(last, Err("instance limit reached".to_string()));
}

#[test]
fn set_param_stores_clamped_values() {
    let mut h = harness(&temp_dir("param"));
    let id = first_id(&h, Kind::Synth);
    let cut = SYNTH_PARAMS.iter().find(|s| s.name == "filt_cutoff").unwrap();
    let mid = (cut.lo * cut.hi).sqrt();
    call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": mid }),
    )
    .unwrap();
    assert!((param(&mut h, id, "filt_cutoff") - mid).abs() < 1e-3 * mid);
    call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": 1e9 }),
    )
    .unwrap();
    assert_eq!(param(&mut h, id, "filt_cutoff"), cut.hi);
    call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": -1e9 }),
    )
    .unwrap();
    assert_eq!(param(&mut h, id, "filt_cutoff"), cut.lo);
}

#[test]
fn set_param_errors() {
    let mut h = harness(&temp_dir("paramerr"));
    let id = first_id(&h, Kind::Synth);
    let e = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "nope", "value": 1 }),
    )
    .unwrap_err();
    for s in SYNTH_PARAMS.iter().filter(|s| s.name != "master_volume") {
        assert!(e.contains(s.name), "{e} lacks {}", s.name);
    }
    let e = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "master_volume", "value": 1 }),
    )
    .unwrap_err();
    assert!(e.contains("use set_master"), "{e}");
    let e = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": null }),
    )
    .unwrap_err();
    assert!(e.contains("value"), "{e}");
    let e = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": "x" }),
    )
    .unwrap_err();
    assert!(e.contains("value"), "{e}");
    assert!(call(&mut h, "set_param", Value::Null).is_err());
    let d = first_id(&h, Kind::Drums);
    assert_eq!(
        call(
            &mut h,
            "set_param",
            json!({ "id": d, "name": "filt_cutoff", "value": 1 })
        ),
        Err(format!("instrument {d} is drums, not a synth"))
    );
    assert_eq!(
        call(
            &mut h,
            "set_param",
            json!({ "id": 999, "name": "filt_cutoff", "value": 1 })
        ),
        Err("no instrument with id 999".to_string())
    );
}

#[test]
fn master_level_mute() {
    let mut h = harness(&temp_dir("mix"));
    call(&mut h, "set_master", json!({ "value": 0.3 })).unwrap();
    let s = call(&mut h, "get_state", json!({})).unwrap();
    assert!((s["master"].as_f64().unwrap() - 0.3).abs() < 1e-6);
    assert!(call(&mut h, "set_master", json!({ "value": null })).is_err());
    assert!(call(&mut h, "set_master", json!({})).is_err());
    let id = first_id(&h, Kind::Synth);
    call(&mut h, "set_level", json!({ "id": id, "value": 0.25 })).unwrap();
    let s = call(&mut h, "get_state", json!({})).unwrap();
    let i = s["instruments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap();
    assert!((i["level"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!(call(&mut h, "set_level", json!({ "id": id })).is_err());
    for _ in 0..2 {
        call(&mut h, "set_mute", json!({ "id": id, "muted": true })).unwrap();
        assert!(h.state().app.model.muted(id));
    }
    call(&mut h, "set_mute", json!({ "id": id, "muted": false })).unwrap();
    assert!(!h.state().app.model.muted(id));
    assert!(call(&mut h, "set_mute", json!({ "id": id })).is_err());
}

#[test]
fn transport_is_idempotent_and_followers_error() {
    let mut h = harness(&temp_dir("transport"));
    let id = h.state().app.model.instruments()[0].id;
    let running = |h: &Harness<'static, S>| {
        h.state()
            .app
            .model
            .view(id)
            .unwrap()
            .shared
            .clock_running
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    call(&mut h, "stop", json!({ "id": id })).unwrap();
    assert!(!running(&h));
    call(&mut h, "play", json!({ "id": id })).unwrap();
    call(&mut h, "play", json!({ "id": id })).unwrap();
    assert!(running(&h));
    call(&mut h, "set_bpm", json!({ "id": id, "bpm": 140 })).unwrap();
    call(&mut h, "stop", json!({ "id": id })).unwrap();
    call(&mut h, "stop", json!({ "id": id })).unwrap();
    assert!(!running(&h));
    assert!(call(&mut h, "set_bpm", json!({ "id": id })).is_err());

    let other = h.state().app.model.instruments()[1].id;
    assert_eq!(
        call(&mut h, "set_source", json!({ "id": other, "source": id })),
        Ok(json!({}))
    );
    let msg = format!("instrument {other} follows instrument {id}'s clock; control {id} instead");
    for (t, a) in [
        ("play", json!({})),
        ("stop", json!({})),
        ("set_bpm", json!({ "bpm": 120 })),
    ] {
        let mut a = a;
        a["id"] = json!(other);
        assert_eq!(call(&mut h, t, a), Err(msg.clone()), "{t}");
    }
    assert!(
        call(
            &mut h,
            "set_source",
            json!({ "id": other, "source": other })
        )
        .is_err()
    );
    call(&mut h, "set_source", json!({ "id": other, "source": null })).unwrap();
    call(&mut h, "play", json!({ "id": other })).unwrap();
}
