//! MCP tools: descriptors and the UI-thread dispatcher (grows task by task).

use std::time::Instant;

use nomad_core::engine::InstrumentId;
use std::sync::atomic::Ordering::Relaxed;

use nomad_core::engine::Kind;
use nomad_core::params::{DRUM_PARAMS, ParamSpec, SYNTH_PARAMS};
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
