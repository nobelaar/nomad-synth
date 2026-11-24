//! MCP tools: descriptors and the UI-thread dispatcher (grows task by task).

use std::time::Instant;

use nomad_core::engine::InstrumentId;
use std::sync::atomic::Ordering::Relaxed;

use nomad_core::engine::Kind;
use nomad_core::params::{DRUM_PARAMS, ParamSpec, SYNTH_PARAMS};
use nomad_runtime::Command;
use serde_json::{Value, json};

use crate::model::InstrumentView;
use crate::presets::{self, Origin, PresetKind};
use crate::ui::NomadApp;

/// A note-off owed to `target` (the synth that got the note-on) at `due`.
pub struct PendingOff {
    pub due: Instant,
    pub note: u8,
    pub target: Option<InstrumentId>,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

pub fn descriptors() -> Vec<Value> {
    let id = json!({ "type": "integer", "description": "instrument id from get_state" });
    let kind = json!({ "type": "string", "enum": ["synth", "drums"] });
    vec![
        tool(
            "get_state",
            "Whole-app snapshot: master, selected/target instrument, telemetry and every instrument.",
            json!({}),
            &[],
        ),
        tool(
            "get_instrument",
            "Full state of one instrument (params and sequence) as a preset JSON.",
            json!({ "id": { "type": "integer", "description": "instrument id from get_state" } }),
            &["id"],
        ),
        tool(
            "list_params",
            "Parameter names with range, default and unit for a kind of instrument.",
            json!({ "kind": kind }),
            &["kind"],
        ),
        tool(
            "list_presets",
            "Preset names (factory and user) for a kind of instrument.",
            json!({ "kind": kind }),
            &["kind"],
        ),
        tool(
            "add_instrument",
            "Add an instrument; it becomes the selection. Returns its id.",
            json!({ "kind": kind }),
            &["kind"],
        ),
        tool(
            "close_instrument",
            "Close an instrument.",
            json!({ "id": id }),
            &["id"],
        ),
        tool(
            "select_instrument",
            "Select an instrument.",
            json!({ "id": id }),
            &["id"],
        ),
        tool(
            "set_target",
            "Make a synth the target of the keyboard/MIDI notes.",
            json!({ "id": id }),
            &["id"],
        ),
        tool(
            "set_source",
            "Clock source of an instrument: another instrument's id, or null for its own clock.",
            json!({ "id": id, "source": { "type": ["integer", "null"] } }),
            &["id", "source"],
        ),
        tool(
            "set_param",
            "Set a synth parameter (see list_params); the value is clamped to its range.",
            json!({ "id": id, "name": { "type": "string" }, "value": { "type": "number" } }),
            &["id", "name", "value"],
        ),
        tool(
            "set_master",
            "Master volume.",
            json!({ "value": { "type": "number" } }),
            &["value"],
        ),
        tool(
            "set_level",
            "Mix level of an instrument (0..1).",
            json!({ "id": id, "value": { "type": "number" } }),
            &["id", "value"],
        ),
        tool(
            "set_mute",
            "Mute or unmute an instrument.",
            json!({ "id": id, "muted": { "type": "boolean" } }),
            &["id", "muted"],
        ),
        tool(
            "set_bpm",
            "Tempo of an instrument with its own clock.",
            json!({ "id": id, "bpm": { "type": "number" } }),
            &["id", "bpm"],
        ),
        tool(
            "play",
            "Start the clock (no-op if running).",
            json!({ "id": id }),
            &["id"],
        ),
        tool(
            "stop",
            "Stop the clock (no-op if stopped).",
            json!({ "id": id }),
            &["id"],
        ),
    ]
}

pub fn call(
    app: &mut NomadApp,
    _offs: &mut Vec<PendingOff>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let empty = json!({});
    let args = if args.is_null() { &empty } else { args };
    match name {
        "get_state" => get_state(app),
        "get_instrument" => get_instrument(app, args),
        "list_params" => list_params(args),
        "list_presets" => list_presets(app, args),
        "add_instrument" => add_instrument(app, args),
        "close_instrument" => {
            let id = arg_id(app, args)?.id;
            app.model.close_instrument(id);
            Ok(json!({}))
        }
        "select_instrument" => {
            let id = arg_id(app, args)?.id;
            app.model.select(id);
            Ok(json!({}))
        }
        "set_target" => {
            let id = synth_id(app, args)?;
            app.model.set_target(id);
            Ok(json!({}))
        }
        "set_source" => set_source(app, args),
        "set_param" => set_param(app, args),
        "set_master" => {
            app.set_master(arg_f64(args, "value")?);
            Ok(json!({}))
        }
        "set_level" => {
            let id = arg_id(app, args)?.id;
            app.model.set_level(id, arg_f64(args, "value")?);
            Ok(json!({}))
        }
        "set_mute" => set_mute(app, args),
        "set_bpm" => {
            let id = arg_id(app, args)?.id;
            let bpm = arg_f64(args, "bpm")?;
            transport(app, id, |m| m.set_bpm(id, bpm))
        }
        "play" | "stop" => {
            let id = arg_id(app, args)?.id;
            let want = name == "play";
            transport(app, id, |m| {
                let running = m
                    .view(id)
                    .is_some_and(|v| v.shared.clock_running.load(Relaxed));
                running == want || m.toggle_play(id)
            })
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

/// The live instance named by `args.id`; unknown or closing ones are "no instrument".
pub fn arg_id<'a>(app: &'a NomadApp, args: &Value) -> Result<&'a InstrumentView, String> {
    let id = args
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("missing or invalid \"id\": expected an instrument id (integer) from get_state")?;
    u32::try_from(id)
        .ok()
        .and_then(|i| app.model.view(i))
        .filter(|v| !v.shared.closing.load(Relaxed))
        .ok_or_else(|| format!("no instrument with id {id}"))
}

fn arg_kind(args: &Value) -> Result<Kind, String> {
    match args.get("kind").and_then(Value::as_str) {
        Some("synth") => Ok(Kind::Synth),
        Some("drums") => Ok(Kind::Drums),
        _ => Err("missing or invalid \"kind\": must be \"synth\" or \"drums\"".into()),
    }
}

fn kind_str(k: Kind) -> &'static str {
    PresetKind::of(k).as_str()
}

fn get_state(app: &NomadApp) -> Result<Value, String> {
    let m = &app.model;
    let t = &app.telemetry;
    let instruments: Vec<Value> = m
        .instruments()
        .iter()
        .filter(|v| !v.shared.closing.load(Relaxed))
        .map(|v| {
            json!({
                "id": v.id, "kind": kind_str(v.kind), "name": v.name, "source": v.source,
                "bpm": v.shared.clock_bpm.get(), "running": v.shared.eff_running.load(Relaxed),
                "muted": m.muted(v.id), "level": v.shared.level.get(),
                "step": v.shared.step.load(Relaxed),
            })
        })
        .collect();
    Ok(json!({
        "master": app.master_value(),
        "selected": m.selected(),
        "target": m.target_synth(),
        "spiral_open": app.spiral.is_some(),
        "telemetry": {
            "peak": f32::from_bits(t.peak.load(Relaxed)),
            "voices": t.voices_total.load(Relaxed),
            "xruns": t.xruns.load(Relaxed),
            "dropped_commands": t.dropped_commands.load(Relaxed),
        },
        "instruments": instruments,
    }))
}

fn get_instrument(app: &NomadApp, args: &Value) -> Result<Value, String> {
    let v = arg_id(app, args)?;
    Ok(presets::capture(v, &v.name))
}

fn specs(list: &[ParamSpec]) -> Value {
    list.iter()
        .map(|s| json!({ "name": s.name, "lo": s.lo, "hi": s.hi, "default": s.default, "unit": s.unit }))
        .collect()
}

fn list_params(args: &Value) -> Result<Value, String> {
    Ok(match arg_kind(args)? {
        Kind::Synth => specs(&SYNTH_PARAMS),
        Kind::Drums => specs(&DRUM_PARAMS),
    })
}

fn list_presets(app: &NomadApp, args: &Value) -> Result<Value, String> {
    let kind = PresetKind::of(arg_kind(args)?);
    Ok(app
        .presets
        .list(kind)
        .into_iter()
        .map(|(name, o)| {
            json!({ "name": name, "origin": if o == Origin::Factory { "factory" } else { "user" } })
        })
        .collect())
}

fn arg_f64(args: &Value, key: &str) -> Result<f64, String> {
    args.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("missing or invalid \"{key}\": {key} must be a finite number"))
}

/// Id of a live synth; drums get a "not a synth" error.
fn synth_id(app: &NomadApp, args: &Value) -> Result<u32, String> {
    let v = arg_id(app, args)?;
    match v.kind {
        Kind::Synth => Ok(v.id),
        Kind::Drums => Err(format!("instrument {} is drums, not a synth", v.id)),
    }
}

fn add_instrument(app: &mut NomadApp, args: &Value) -> Result<Value, String> {
    let kind = arg_kind(args)?;
    let id = app
        .model
        .add_instrument(kind)
        .ok_or("instance limit reached")?;
    Ok(json!({ "id": id }))
}

fn set_source(app: &mut NomadApp, args: &Value) -> Result<Value, String> {
    let id = arg_id(app, args)?.id;
    let src = match args.get("source") {
        Some(Value::Null) => None,
        Some(s) => Some(
            s.as_u64()
                .and_then(|s| u32::try_from(s).ok())
                .ok_or("invalid \"source\": expected an instrument id (integer) or null")?,
        ),
        None => return Err("missing \"source\": an instrument id (integer) or null".into()),
    };
    if app.model.set_source(id, src) {
        Ok(json!({}))
    } else {
        Err(format!(
            "invalid clock source for instrument {id}: the source must be another live instrument with its own clock, and an instrument that others follow cannot follow"
        ))
    }
}

fn set_param(app: &mut NomadApp, args: &Value) -> Result<Value, String> {
    let id = synth_id(app, args)?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing or invalid \"name\": expected a parameter name (see list_params)")?;
    if name == "master_volume" {
        return Err("master_volume is not an instrument parameter: use set_master".into());
    }
    let Some((p, spec)) = SYNTH_PARAMS
        .iter()
        .enumerate()
        .find(|(_, s)| s.name == name)
    else {
        let valid: Vec<_> = SYNTH_PARAMS
            .iter()
            .map(|s| s.name)
            .filter(|n| *n != "master_volume")
            .collect();
        return Err(format!(
            "unknown parameter \"{name}\"; valid names: {}",
            valid.join(", ")
        ));
    };
    let value = spec.clamp(arg_f64(args, "value")?);
    app.model.send(Command::SetParam {
        id,
        param: p as u16,
        value,
    });
    Ok(json!({ "value": value }))
}

fn set_mute(app: &mut NomadApp, args: &Value) -> Result<Value, String> {
    let id = arg_id(app, args)?.id;
    let want = args
        .get("muted")
        .and_then(Value::as_bool)
        .ok_or("missing or invalid \"muted\": must be true or false")?;
    if app.model.muted(id) != want {
        app.model.toggle_mute(id);
    }
    Ok(json!({}))
}

/// Run a transport action; `false` means the instance follows another clock.
fn transport(
    app: &mut NomadApp,
    id: u32,
    f: impl FnOnce(&mut crate::model::AppModel) -> bool,
) -> Result<Value, String> {
    if let Some(m) = app.model.source_of(id) {
        return Err(format!(
            "instrument {id} follows instrument {m}'s clock; control {m} instead"
        ));
    }
    f(&mut app.model);
    Ok(json!({}))
}
