//! MCP tools run on the UI thread against a real `NomadApp`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::Vec2;
use egui_kittest::Harness;
use nomad_app::mcp::tools;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::presets::{self, PresetKind, PresetStore};
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
    let cut = SYNTH_PARAMS
        .iter()
        .find(|s| s.name == "filt_cutoff")
        .unwrap();
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

fn id_of(h: &mut Harness<'static, S>, kind: &str) -> u64 {
    let st = call(h, "get_state", json!({})).unwrap();
    st["instruments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind)
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn voices(h: &Harness<'static, S>) -> u32 {
    h.state()
        .app
        .telemetry
        .voices_total
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// `note_on` through the tool, keeping the pending offs for `flush_offs`.
fn note_on(
    h: &mut Harness<'static, S>,
    offs: &mut Vec<tools::PendingOff>,
    args: Value,
) -> Result<Value, String> {
    let r = tools::call(&mut h.state_mut().app, offs, "note_on", &args);
    step(h);
    r
}

fn flush(h: &mut Harness<'static, S>, offs: &mut Vec<tools::PendingOff>, now: std::time::Instant) {
    nomad_app::mcp::flush_offs(offs, &mut h.state_mut().app, now);
    step(h);
}

#[test]
fn note_on_sounds_then_off_when_due() {
    let mut h = harness(&temp_dir("noteon"));
    let mut offs = Vec::new();
    note_on(&mut h, &mut offs, json!({ "note": 60 })).unwrap();
    assert!(voices(&h) >= 1);
    let now = std::time::Instant::now();
    flush(&mut h, &mut offs, now);
    assert_eq!(offs.len(), 1, "not due yet");
    flush(&mut h, &mut offs, now + std::time::Duration::from_secs(11));
    assert!(offs.is_empty());
    // release tail: render until the voice dies
    for _ in 0..200 {
        step(&mut h);
    }
    assert_eq!(voices(&h), 0);
}

#[test]
fn note_on_validates() {
    let mut h = harness(&temp_dir("noteonbad"));
    let mut offs = Vec::new();
    let e = note_on(&mut h, &mut offs, json!({ "note": 200 })).unwrap_err();
    assert!(e.contains("0 to 127"), "{e}");
    assert!(note_on(&mut h, &mut offs, json!({ "note": 60, "velocity": 2 })).is_err());
    assert!(note_on(&mut h, &mut offs, json!({ "note": 60, "duration_ms": 0 })).is_err());
    assert!(
        note_on(
            &mut h,
            &mut offs,
            json!({ "note": 60, "duration_ms": 1000000000u64 })
        )
        .is_err()
    );
    assert!(offs.is_empty() && voices(&h) == 0);
}

#[test]
fn note_on_without_a_synth_errors() {
    let mut h = harness(&temp_dir("nosynth"));
    let synth = id_of(&mut h, "synth");
    call(&mut h, "close_instrument", json!({ "id": synth })).unwrap();
    let mut offs = Vec::new();
    assert_eq!(
        note_on(&mut h, &mut offs, json!({ "note": 60 })),
        Err("no synth to play: add a synth first".to_string())
    );
}

fn running(h: &Harness<'static, S>, id: u64) -> bool {
    h.state()
        .app
        .model
        .view(id as u32)
        .unwrap()
        .shared
        .clock_running
        .load(std::sync::atomic::Ordering::Relaxed)
}

fn due() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(11)
}

#[test]
fn retargeted_note_is_released_without_stopping_other_clocks() {
    let mut h = harness(&temp_dir("retarget"));
    let drums = id_of(&mut h, "drums");
    call(&mut h, "play", json!({ "id": drums })).unwrap();
    step(&mut h);
    assert!(running(&h, drums));
    let mut offs = Vec::new();
    note_on(&mut h, &mut offs, json!({ "note": 60 })).unwrap();
    assert!(voices(&h) >= 1);
    let other = call(&mut h, "add_instrument", json!({ "kind": "synth" })).unwrap()["id"].clone();
    call(&mut h, "set_target", json!({ "id": other })).unwrap();
    flush(&mut h, &mut offs, due());
    assert!(offs.is_empty());
    for _ in 0..200 {
        step(&mut h);
    }
    assert_eq!(voices(&h), 0, "the old synth's note was released");
    assert!(running(&h, drums), "other clocks keep running");
    assert_eq!(
        h.state().app.model.target_synth(),
        other.as_u64().map(|i| i as u32)
    );
}

#[test]
fn closed_target_off_is_dropped_without_stopping_other_clocks() {
    let mut h = harness(&temp_dir("closedtarget"));
    let drums = id_of(&mut h, "drums");
    call(&mut h, "play", json!({ "id": drums })).unwrap();
    step(&mut h);
    let mut offs = Vec::new();
    let other = call(&mut h, "add_instrument", json!({ "kind": "synth" })).unwrap()["id"].clone();
    call(&mut h, "set_target", json!({ "id": other })).unwrap();
    note_on(&mut h, &mut offs, json!({ "note": 64 })).unwrap();
    call(&mut h, "close_instrument", json!({ "id": other })).unwrap();
    flush(&mut h, &mut offs, due());
    assert!(offs.is_empty());
    assert!(running(&h, drums), "other clocks keep running");
}

#[test]
fn retriggered_note_keeps_one_later_off() {
    let mut h = harness(&temp_dir("retrigger"));
    let mut offs = Vec::new();
    note_on(&mut h, &mut offs, json!({ "note": 60, "duration_ms": 100 })).unwrap();
    let first = offs[0].due;
    note_on(
        &mut h,
        &mut offs,
        json!({ "note": 60, "duration_ms": 5000 }),
    )
    .unwrap();
    assert_eq!(offs.len(), 1);
    assert!(offs[0].due > first + std::time::Duration::from_secs(1));
    flush(
        &mut h,
        &mut offs,
        first + std::time::Duration::from_millis(200),
    );
    assert_eq!(offs.len(), 1, "the first off must not fire");
}

#[test]
fn panic_silences_and_clears_offs() {
    let mut h = harness(&temp_dir("panic"));
    let mut offs = Vec::new();
    note_on(&mut h, &mut offs, json!({ "note": 60 })).unwrap();
    let r = tools::call(&mut h.state_mut().app, &mut offs, "panic", &json!({}));
    step(&mut h);
    r.unwrap();
    assert!(offs.is_empty());
    assert_eq!(voices(&h), 0);
}

#[test]
fn seq_tools_edit_the_pattern() {
    let mut h = harness(&temp_dir("seq"));
    let id = id_of(&mut h, "synth");
    call(
        &mut h,
        "seq_set_step",
        json!({ "id": id, "step": 3, "on": true, "note": 67, "vel": 100 }),
    )
    .unwrap();
    let j = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    assert_eq!(j["seq"]["on"][3], 1);
    assert_eq!(j["seq"]["note"][3], 67);
    assert_eq!(j["seq"]["vel"][3], 100);
    call(
        &mut h,
        "seq_set",
        json!({ "id": id, "length": 8, "swing": 0.2 }),
    )
    .unwrap();
    let j = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    assert_eq!(j["seq"]["length"], 8);
    call(&mut h, "seq_clear", json!({ "id": id })).unwrap();
    let j = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    assert_eq!(j["seq"]["on"][3], 0);
    for bad in [
        json!({ "id": id, "step": 16, "on": true }),
        json!({ "id": id, "step": 1, "on": true, "note": 128 }),
        json!({ "id": id, "step": 1, "on": true, "vel": 0 }),
        json!({ "id": id, "step": 1 }),
    ] {
        assert!(call(&mut h, "seq_set_step", bad).is_err());
    }
    assert!(call(&mut h, "seq_set", json!({ "id": id })).is_err());
    assert!(call(&mut h, "seq_set", json!({ "id": id, "length": 17 })).is_err());
    assert!(call(&mut h, "seq_set", json!({ "id": id, "gate": "x" })).is_err());
}

#[test]
fn drum_tools_edit_and_trigger() {
    let mut h = harness(&temp_dir("drums"));
    let id = id_of(&mut h, "drums");
    call(
        &mut h,
        "drum_set_step",
        json!({ "id": id, "track": 2, "step": 5, "level": 2 }),
    )
    .unwrap();
    let j = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    assert_eq!(j["seq"]["steps"][2][5], 2);
    call(&mut h, "drum_clear", json!({ "id": id })).unwrap();
    let j = call(&mut h, "get_instrument", json!({ "id": id })).unwrap();
    assert_eq!(j["seq"]["steps"][2][5], 0);
    call(&mut h, "drum_trigger", json!({ "id": id, "track": 0 })).unwrap();
    assert!(call(&mut h, "drum_trigger", json!({ "id": id, "track": 6 })).is_err());
    assert!(
        call(
            &mut h,
            "drum_set_step",
            json!({ "id": id, "track": 0, "step": 16, "level": 1 })
        )
        .is_err()
    );
    assert!(
        call(
            &mut h,
            "drum_set_step",
            json!({ "id": id, "track": 0, "step": 0, "level": 3 })
        )
        .is_err()
    );
}

#[test]
fn seq_and_drum_tools_reject_the_wrong_kind() {
    let mut h = harness(&temp_dir("kinds"));
    let (s, d) = (id_of(&mut h, "synth"), id_of(&mut h, "drums"));
    for (t, a) in [
        (
            "drum_set_step",
            json!({ "track": 0, "step": 0, "level": 1 }),
        ),
        ("drum_clear", json!({})),
        ("drum_trigger", json!({ "track": 0 })),
    ] {
        let mut a = a;
        a["id"] = json!(s);
        assert_eq!(
            call(&mut h, t, a),
            Err(format!("instrument {s} is a synth, not drums")),
            "{t}"
        );
    }
    for (t, a) in [
        ("seq_set_step", json!({ "step": 0, "on": true })),
        ("seq_clear", json!({})),
        ("seq_set", json!({ "length": 4 })),
    ] {
        let mut a = a;
        a["id"] = json!(d);
        assert_eq!(
            call(&mut h, t, a),
            Err(format!("instrument {d} is drums, not a synth")),
            "{t}"
        );
    }
    assert_eq!(
        call(&mut h, "seq_clear", json!({ "id": 999 })),
        Err("no instrument with id 999".to_string())
    );
}

#[test]
fn seq_enable_toggles_and_get_state_reports_it() {
    let mut h = harness(&temp_dir("seqen"));
    let id = call(&mut h, "add_instrument", json!({ "kind": "synth" })).unwrap()["id"].clone();
    let enabled = |h: &mut _| {
        let st = call(h, "get_state", json!({})).unwrap();
        st["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .unwrap()["seq_enabled"]
            .clone()
    };
    call(&mut h, "seq_enable", json!({ "id": id, "enabled": true })).unwrap();
    step(&mut h);
    assert_eq!(enabled(&mut h), json!(true));
    call(&mut h, "seq_enable", json!({ "id": id, "enabled": false })).unwrap();
    step(&mut h);
    assert_eq!(enabled(&mut h), json!(false));
    assert!(call(&mut h, "seq_enable", json!({ "id": id })).is_err());
}

#[test]
fn every_descriptor_is_dispatched() {
    let mut h = harness(&temp_dir("drift"));
    for d in tools::descriptors() {
        let name = d["name"].as_str().unwrap();
        if let Err(e) = call(&mut h, name, json!({})) {
            assert!(
                !e.starts_with("unknown tool"),
                "{name} has a descriptor but no handler"
            );
        }
    }
}

#[test]
fn cross_target_retrigger_keeps_both_offs() {
    let mut h = harness(&temp_dir("crossretrigger"));
    let mut offs = Vec::new();
    let a = id_of(&mut h, "synth");
    let b = call(&mut h, "add_instrument", json!({ "kind": "synth" })).unwrap()["id"].clone();
    call(&mut h, "set_target", json!({ "id": a })).unwrap();
    note_on(&mut h, &mut offs, json!({ "note": 60 })).unwrap();
    call(&mut h, "set_target", json!({ "id": b })).unwrap();
    note_on(&mut h, &mut offs, json!({ "note": 60 })).unwrap();
    assert_eq!(offs.len(), 2, "A's off must survive the retrigger on B");
    flush(
        &mut h,
        &mut offs,
        std::time::Instant::now() + std::time::Duration::from_secs(11),
    );
    assert!(offs.is_empty());
    for _ in 0..200 {
        step(&mut h);
    }
    assert_eq!(voices(&h), 0, "A's note must not stay stuck");
}

fn params_of(h: &mut Harness<'static, S>, id: u64) -> Value {
    call(h, "get_instrument", json!({ "id": id })).unwrap()["params"].clone()
}

#[test]
fn load_preset_applies_factory_and_reports_unknown() {
    let mut h = harness(&temp_dir("loadpreset"));
    let id = id_of(&mut h, "synth");
    let before = params_of(&mut h, id);
    let list = call(&mut h, "list_presets", json!({ "kind": "synth" })).unwrap();
    let name = list.as_array().unwrap().iter().find_map(|e| {
        let n = e["name"].as_str().unwrap();
        n.contains("Dark techno").then(|| n.to_string())
    });
    let name = name.unwrap();
    call(&mut h, "load_preset", json!({ "id": id, "name": name })).unwrap();
    assert_ne!(params_of(&mut h, id), before);
    assert_eq!(h.state().app.preset_names[&(id as u32)], name);
    let e = call(
        &mut h,
        "load_preset",
        json!({ "id": id, "name": "Nope123" }),
    )
    .unwrap_err();
    assert!(e.contains("Nope123"), "{e}");
    assert!(call(&mut h, "load_preset", json!({ "id": id })).is_err());
    assert!(
        call(&mut h, "load_preset", json!({ "id": 999, "name": "x" }))
            .unwrap_err()
            .contains("no instrument with id 999")
    );
}

#[test]
fn save_preset_round_trips_and_validates() {
    let dir = temp_dir("savepreset");
    let mut h = harness(&dir);
    let id = id_of(&mut h, "synth");
    let factory_name = presets::factory(PresetKind::Synth)[0]["name"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": 777.0 }),
    )
    .unwrap();
    let saved_params = params_of(&mut h, id);
    let r = call(
        &mut h,
        "save_preset",
        json!({ "id": id, "name": "My Patch" }),
    )
    .unwrap();
    assert_eq!(r["saved"], "My Patch");
    assert!(h.state().app.status.contains("PRESET guardado"));
    let list = call(&mut h, "list_presets", json!({ "kind": "synth" })).unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "My Patch" && e["origin"] == "user")
    );
    call(
        &mut h,
        "load_preset",
        json!({ "id": id, "name": &factory_name }),
    )
    .unwrap();
    assert_ne!(params_of(&mut h, id), saved_params);
    call(
        &mut h,
        "load_preset",
        json!({ "id": id, "name": "My Patch" }),
    )
    .unwrap();
    assert_eq!(params_of(&mut h, id), saved_params);
    for bad in ["", "   ", ".."] {
        assert!(
            call(&mut h, "save_preset", json!({ "id": id, "name": bad })).is_err(),
            "{bad:?}"
        );
    }
    assert!(call(&mut h, "save_preset", json!({ "id": id })).is_err());
    let e = call(
        &mut h,
        "save_preset",
        json!({ "id": id, "name": factory_name.to_uppercase() }),
    )
    .unwrap_err();
    assert!(e.contains("factory preset names are reserved"), "{e}");
}

#[test]
fn save_preset_without_folder_errors() {
    let mut h = harness(&temp_dir("nofolder"));
    h.state_mut().app.presets = PresetStore::new(None);
    let id = id_of(&mut h, "synth");
    let e = call(&mut h, "save_preset", json!({ "id": id, "name": "x" })).unwrap_err();
    assert!(e.contains("no preset folder"), "{e}");
}

fn all_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            all_files(&p, out)
        } else {
            out.push(p)
        }
    }
}

#[test]
fn save_preset_names_cannot_escape_the_folder() {
    let root = temp_dir("traversal");
    let dir = root.join("presets");
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness(&dir);
    let id = id_of(&mut h, "synth");
    let long = "a".repeat(5000);
    for name in [
        "../../escape",
        "a/b",
        "..",
        "/etc/passwd",
        "",
        &long,
        "..\\..\\x",
        "a\0b",
    ] {
        let _ = call(&mut h, "save_preset", json!({ "id": id, "name": name }));
    }
    let mut files = Vec::new();
    all_files(&root, &mut files);
    assert!(!files.is_empty());
    for f in files {
        assert!(f.starts_with(&dir), "{f:?} escaped");
        assert!(f.file_name().unwrap().len() < 60);
    }
}

#[test]
fn spiral_tools_toggle_and_are_idempotent() {
    let mut h = harness(&temp_dir("spiral"));
    for (tool, want) in [
        ("open_spiral", true),
        ("open_spiral", true),
        ("close_spiral", false),
        ("close_spiral", false),
    ] {
        call(&mut h, tool, json!({})).unwrap();
        let st = call(&mut h, "get_state", json!({})).unwrap();
        assert_eq!(st["spiral_open"], want, "{tool}");
    }
}

#[test]
fn set_param_rounds_selectors_only() {
    let mut h = harness(&temp_dir("selround"));
    let id = first_id(&h, Kind::Synth);
    let sel = SYNTH_PARAMS
        .iter()
        .enumerate()
        .find(|(p, _)| {
            nomad_app::synth_layout::widget_kind(*p)
                == nomad_app::synth_layout::WidgetKind::Selector
        })
        .unwrap()
        .1
        .name;
    let r = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": sel, "value": 2.7 }),
    )
    .unwrap();
    assert_eq!(r["value"], 3.0);
    step(&mut h);
    assert_eq!(param(&mut h, id, sel), 3.0);
    let r = call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": 1234.5 }),
    )
    .unwrap();
    assert_eq!(r["value"], 1234.5);
}

#[test]
fn save_preset_does_not_overwrite_without_flag() {
    let dir = temp_dir("overwrite");
    let mut h = harness(&dir);
    let id = id_of(&mut h, "synth");
    call(&mut h, "save_preset", json!({ "id": id, "name": "Dup" })).unwrap();
    let file = dir.join("synth").join("Dup.json");
    let before = std::fs::read_to_string(&file).unwrap();
    call(
        &mut h,
        "set_param",
        json!({ "id": id, "name": "filt_cutoff", "value": 555.0 }),
    )
    .unwrap();
    step(&mut h);
    let e = call(&mut h, "save_preset", json!({ "id": id, "name": "Dup" })).unwrap_err();
    assert_eq!(
        e,
        "a user preset named 'Dup' already exists; pass overwrite: true to replace it"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    call(
        &mut h,
        "save_preset",
        json!({ "id": id, "name": "Dup", "overwrite": true }),
    )
    .unwrap();
    assert_ne!(std::fs::read_to_string(&file).unwrap(), before);
    call(&mut h, "save_preset", json!({ "id": id, "name": "Other" })).unwrap();
    let e = call(
        &mut h,
        "save_preset",
        json!({ "id": id, "name": "Dup", "overwrite": "yes" }),
    )
    .unwrap_err();
    assert!(e.contains("overwrite"), "{e}");
}
