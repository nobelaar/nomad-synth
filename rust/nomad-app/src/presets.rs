//! Instrument presets, ported from `nomad/presets.py`: sound plus pattern,
//! factory (in code) and user (JSON files in `<config>/presets/<kind>/`).
//!
//! User files are untrusted: `check` validates the whole preset before
//! `apply_commands` builds anything, so a bad file changes nothing. Tempo, level,
//! mute, clock source and SEQ on/off are session state and never part of a preset.
//! Files are written byte for byte like Python's `json.dumps(data, indent=1)`.

use std::path::{Path, PathBuf};

use nomad_core::clock::STEPS;
use nomad_core::drum_synth::render_one_shot;
use nomad_core::engine::Kind;
use nomad_core::mirror::{DrumSnapshot, SeqSnapshot};
use nomad_core::params::{DRUM_PARAMS, INSTRUMENTS, SYNTH_PARAMS, pid};
use nomad_core::sequencer::{EXTRA, NONE, SeqPattern};
use nomad_runtime::Command;
use serde_json::{Map, Value, json};

use crate::model::InstrumentView;

pub const VERSION: u64 = 1;
pub const MAX_BYTES: u64 = 64 * 1024;
const TRACKS: usize = INSTRUMENTS.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetKind {
    Synth,
    Drums,
}

impl PresetKind {
    pub fn of(kind: Kind) -> PresetKind {
        match kind {
            Kind::Synth => PresetKind::Synth,
            Kind::Drums => PresetKind::Drums,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PresetKind::Synth => "synth",
            PresetKind::Drums => "drums",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Factory,
    User,
}

// ---- validation -------------------------------------------------------------------------

/// `_ints`: a list of `n` integers (not floats, not bools) in `lo..=hi`.
fn ints(x: Option<&Value>, n: usize, lo: i64, hi: i64) -> bool {
    matches!(x, Some(Value::Array(a)) if a.len() == n
        && a.iter().all(|v| v.as_i64().is_some_and(|i| (lo..=hi).contains(&i))))
}

/// `check` of `nomad/presets.py`, with the same messages.
pub fn check(kind: PresetKind, data: &Value) -> Result<(), String> {
    let not = || format!("not a {} preset", kind.as_str());
    let obj = data.as_object().ok_or_else(not)?;
    if obj.get("kind").and_then(Value::as_str) != Some(kind.as_str()) {
        return Err(not());
    }
    let (Some(Value::Object(params)), Some(Value::Object(seq))) =
        (obj.get("params"), obj.get("seq"))
    else {
        return Err("missing params/seq".into());
    };
    // serde_json numbers are always finite (NaN does not parse).
    if !params.values().all(Value::is_number) {
        return Err("bad param value".into());
    }
    match kind {
        PresetKind::Synth => {
            let extra_ok = matches!(seq.get("extra"), Some(Value::Array(rows)) if rows.len() == STEPS
            && rows.iter().all(|r| ints(Some(r), EXTRA, 0, i64::from(NONE))
                && r.as_array().is_some_and(|r| r.iter().all(|v| {
                    let v = v.as_i64().unwrap_or(-1);
                    v == i64::from(NONE) || v <= 127
                }))));
            let ok = ints(seq.get("on"), STEPS, 0, 1)
                && ints(seq.get("note"), STEPS, 0, 127)
                && ints(seq.get("vel"), STEPS, 1, 127)
                && extra_ok
                && ["gate", "division", "swing"]
                    .iter()
                    .all(|k| seq.get(*k).is_some_and(Value::is_number))
                && ints(
                    Some(&Value::Array(vec![
                        seq.get("length").cloned().unwrap_or(Value::Null),
                    ])),
                    1,
                    1,
                    STEPS as i64,
                );
            if !ok {
                return Err("bad sequence".into());
            }
        }
        PresetKind::Drums => {
            let ok = matches!(seq.get("steps"), Some(Value::Array(rows)) if rows.len() == TRACKS
                && rows.iter().all(|r| ints(Some(r), STEPS, 0, 2)));
            if !ok {
                return Err("bad pattern".into());
            }
        }
    }
    Ok(())
}

// ---- capture / apply ----------------------------------------------------------------------

/// A preset from the instrument's mirror, with the keys of Python's `capture`.
pub fn capture(inst: &InstrumentView, name: &str) -> Value {
    let kind = PresetKind::of(inst.kind);
    let mut params = Map::new();
    let seq = match kind {
        PresetKind::Synth => {
            for (i, s) in SYNTH_PARAMS.iter().enumerate() {
                if i != pid::MASTER_VOLUME {
                    params.insert(s.name.into(), json!(inst.shared.param(i)));
                }
            }
            let mut q = SeqSnapshot::default();
            inst.shared.seq_snapshot(&mut q);
            json!({
                "on": q.on, "note": q.note, "vel": q.vel, "extra": q.extra,
                "gate": q.gate, "division": q.division, "length": q.length, "swing": q.swing,
            })
        }
        PresetKind::Drums => {
            let mut d = DrumSnapshot::default();
            inst.shared.drum_snapshot(&mut d);
            for (s, v) in DRUM_PARAMS.iter().zip(d.values) {
                if s.name != "bpm" {
                    params.insert(s.name.into(), json!(v));
                }
            }
            json!({ "steps": d.steps })
        }
    };
    json!({
        "version": VERSION, "kind": kind.as_str(), "name": name,
        "params": params, "seq": seq,
    })
}

fn u8_row<const N: usize>(v: &Value) -> [u8; N] {
    let a = v.as_array();
    std::array::from_fn(|i| {
        a.and_then(|a| a.get(i))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u8
    })
}

/// The commands that load `preset` into `inst` (validated first; unknown names
/// skipped). Drums: the six one-shots are rendered here, off the audio thread.
pub fn apply_commands(
    inst: &InstrumentView,
    preset: &Value,
    sample_rate: f64,
) -> Result<Vec<Command>, String> {
    let kind = PresetKind::of(inst.kind);
    check(kind, preset)?;
    let (params, seq) = (&preset["params"], &preset["seq"]);
    let id = inst.id;
    let mut out = Vec::new();
    let values = params.as_object().into_iter().flatten();
    match kind {
        PresetKind::Synth => {
            for (n, v) in values {
                if let Some(i) = SYNTH_PARAMS.iter().position(|s| s.name == n)
                    && i != pid::MASTER_VOLUME
                {
                    out.push(Command::SetParam {
                        id,
                        param: i as u16,
                        value: v.as_f64().unwrap_or_default(),
                    });
                }
            }
            let extra = seq["extra"].as_array().expect("checked");
            out.push(Command::SeqLoadPattern {
                id,
                pattern: Box::new(SeqPattern {
                    on: u8_row(&seq["on"]),
                    note: u8_row(&seq["note"]),
                    vel: u8_row(&seq["vel"]),
                    extra: std::array::from_fn(|k| u8_row(&extra[k])),
                    gate: seq["gate"].as_f64().unwrap_or(0.5),
                    division: seq["division"].as_f64().unwrap_or(0.25),
                    length: seq["length"].as_u64().unwrap_or(16) as u8,
                    swing: seq["swing"].as_f64().unwrap_or(0.0),
                }),
            });
        }
        PresetKind::Drums => {
            let mut now = DrumSnapshot::default();
            inst.shared.drum_snapshot(&mut now);
            let mut vals = now.values;
            for (n, v) in values {
                if let Some(i) = DRUM_PARAMS.iter().position(|s| s.name == n)
                    && n != "bpm"
                {
                    let v = v.as_f64().unwrap_or_default();
                    vals[i] = DRUM_PARAMS[i].clamp(v);
                    out.push(Command::DrumValue {
                        id,
                        param: i as u16,
                        value: v,
                    });
                }
            }
            let rows = seq["steps"].as_array().expect("checked");
            out.push(Command::DrumLoadPattern {
                id,
                steps: std::array::from_fn(|t| u8_row(&rows[t])),
            });
            for t in 0..TRACKS {
                out.push(Command::DrumSetBank {
                    id,
                    track: t as u8,
                    shot: Box::new(render_one_shot(
                        t,
                        sample_rate,
                        vals[3 * t],
                        vals[3 * t + 1],
                    )),
                });
            }
        }
    }
    Ok(out)
}

// ---- factory presets ----------------------------------------------------------------------

fn pad<T: Clone>(row: &[T], fill: T) -> Vec<T> {
    let mut v = row.to_vec();
    v.resize(STEPS, fill);
    v
}

fn chord(notes: &[u8]) -> Vec<u8> {
    let mut v = notes.to_vec();
    v.resize(EXTRA, NONE);
    v
}

struct SeqSpec<'a> {
    notes: &'a [u8],
    on: Option<&'a [u8]>,
    vel: Option<&'a [u8]>,
    division: f64,
    length: usize,
    gate: f64,
    extra: Option<Vec<Vec<u8>>>,
}

impl Default for SeqSpec<'_> {
    fn default() -> Self {
        SeqSpec {
            notes: &[],
            on: None,
            vel: None,
            division: 0.25,
            length: STEPS,
            gate: 0.5,
            extra: None,
        }
    }
}

/// `_synth`: every parameter at its default, then `over`.
fn synth(name: &str, over: &[(&str, f64)], s: SeqSpec) -> Value {
    let mut params = Map::new();
    for (i, p) in SYNTH_PARAMS.iter().enumerate() {
        if i != pid::MASTER_VOLUME {
            params.insert(p.name.into(), json!(p.default));
        }
    }
    for (k, v) in over {
        params.insert((*k).into(), json!(v));
    }
    let n = s.notes.len();
    let ones = vec![1u8; n];
    let hundreds = vec![100u8; n];
    let empty = vec![NONE; EXTRA];
    let extra = match s.extra {
        Some(e) => pad(&e, empty),
        None => vec![empty; STEPS],
    };
    json!({
        "version": VERSION, "kind": "synth", "name": name, "params": params,
        "seq": {
            "on": pad(s.on.unwrap_or(&ones), 0), "note": pad(s.notes, 60),
            "vel": pad(s.vel.unwrap_or(&hundreds), 100), "extra": extra,
            "gate": s.gate, "division": s.division, "length": s.length, "swing": 0.0,
        },
    })
}

/// `_drums`: defaults plus `over`, and `(track, [(step, level)])` hits.
fn drums(name: &str, tracks: &[(&str, &[(usize, u8)])], over: &[(&str, f64)]) -> Value {
    let mut params = Map::new();
    for p in DRUM_PARAMS.iter().filter(|p| p.name != "bpm") {
        params.insert(p.name.into(), json!(p.default));
    }
    for (k, v) in over {
        params.insert((*k).into(), json!(v));
    }
    let mut steps = [[0u8; STEPS]; TRACKS];
    for (track, hits) in tracks {
        let t = INSTRUMENTS
            .iter()
            .position(|n| n == track)
            .expect("known track");
        for &(step, level) in *hits {
            steps[t][step] = level;
        }
    }
    json!({
        "version": VERSION, "kind": "drums", "name": name, "params": params,
        "seq": { "steps": steps },
    })
}

/// The factory presets of `FACTORY` (4 synth, 3 drums), in Python's order.
// wave: 0 sin 1 tri 2 saw 3 sqr; modN_src: 1 lfo1 2 lfo2 3 mw 4 vel 5 env2; modN_dst: 1 pitch .. 5 cut
pub fn factory(kind: PresetKind) -> Vec<Value> {
    match kind {
        PresetKind::Synth => vec![
            synth(
                "Pad de acordes",
                &[
                    ("osc1_wave", 2.0),
                    ("osc2_wave", 2.0),
                    ("osc1_fine", -9.0),
                    ("osc2_fine", 9.0),
                    ("osc1_level", 0.45),
                    ("osc2_level", 0.45),
                    ("filt_cutoff", 2200.0),
                    ("filt_reso", 0.2),
                    ("filt_env_amt", 0.25),
                    ("amp_attack", 900.0),
                    ("amp_decay", 800.0),
                    ("amp_sustain", 0.85),
                    ("amp_release", 2500.0),
                    ("filt_attack", 1200.0),
                    ("filt_decay", 1500.0),
                    ("filt_sustain", 0.5),
                    ("filt_release", 2000.0),
                    ("lfo1_wave", 1.0),
                    ("lfo1_rate", 0.25),
                    ("lfo1_depth", 1.0),
                    ("mod0_src", 1.0),
                    ("mod0_dst", 5.0),
                    ("mod0_amt", 0.15),
                    ("polyphony", 6.0),
                ],
                SeqSpec {
                    notes: &[45, 41, 48, 43],
                    vel: Some(&[90; 4]),
                    division: 4.0,
                    length: 4,
                    gate: 1.0,
                    extra: Some(vec![
                        chord(&[48, 52]),
                        chord(&[45, 48]),
                        chord(&[52, 55]),
                        chord(&[47, 50]),
                    ]),
                    ..SeqSpec::default()
                },
            ),
            synth(
                "Bajo",
                &[
                    ("osc1_wave", 2.0),
                    ("osc2_wave", 3.0),
                    ("osc2_coarse", -12.0),
                    ("osc1_level", 0.6),
                    ("osc2_level", 0.4),
                    ("filt_cutoff", 400.0),
                    ("filt_reso", 0.45),
                    ("filt_env_amt", 0.6),
                    ("amp_attack", 2.0),
                    ("amp_decay", 220.0),
                    ("amp_sustain", 0.5),
                    ("amp_release", 120.0),
                    ("filt_attack", 1.0),
                    ("filt_decay", 200.0),
                    ("filt_sustain", 0.1),
                    ("filt_release", 100.0),
                    ("polyphony", 1.0),
                    ("glide", 60.0),
                ],
                SeqSpec {
                    notes: &[36, 36, 39, 36, 34, 41],
                    gate: 0.45,
                    on: Some(&[1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0]),
                    ..SeqSpec::default()
                },
            ),
            synth(
                "Lead",
                &[
                    ("osc1_wave", 3.0),
                    ("osc1_pw", 0.4),
                    ("osc2_wave", 2.0),
                    ("osc2_fine", 7.0),
                    ("osc1_level", 0.5),
                    ("osc2_level", 0.35),
                    ("filt_cutoff", 3500.0),
                    ("filt_reso", 0.25),
                    ("filt_env_amt", 0.3),
                    ("amp_attack", 8.0),
                    ("amp_decay", 250.0),
                    ("amp_sustain", 0.7),
                    ("amp_release", 300.0),
                    ("lfo1_wave", 1.0),
                    ("lfo1_rate", 5.5),
                    ("lfo1_depth", 1.0),
                    ("mod0_src", 1.0),
                    ("mod0_dst", 1.0),
                    ("mod0_amt", 0.04),
                    ("mod1_src", 3.0),
                    ("mod1_dst", 5.0),
                    ("mod1_amt", 0.3),
                    ("polyphony", 1.0),
                    ("glide", 90.0),
                ],
                SeqSpec {
                    notes: &[69, 72, 76, 72, 69, 67, 64, 67],
                    length: 8,
                    gate: 0.8,
                    ..SeqSpec::default()
                },
            ),
            synth(
                "Pluck",
                &[
                    ("osc1_wave", 2.0),
                    ("osc2_wave", 1.0),
                    ("osc2_coarse", 12.0),
                    ("osc1_level", 0.5),
                    ("osc2_level", 0.3),
                    ("filt_cutoff", 600.0),
                    ("filt_reso", 0.35),
                    ("filt_env_amt", 0.8),
                    ("amp_attack", 1.0),
                    ("amp_decay", 350.0),
                    ("amp_sustain", 0.0),
                    ("amp_release", 250.0),
                    ("filt_attack", 1.0),
                    ("filt_decay", 300.0),
                    ("filt_sustain", 0.0),
                    ("polyphony", 6.0),
                ],
                SeqSpec {
                    notes: &[
                        57, 60, 64, 69, 57, 60, 64, 69, 57, 60, 64, 69, 57, 60, 64, 69,
                    ],
                    division: 0.5,
                    gate: 0.5,
                    ..SeqSpec::default()
                },
            ),
        ],
        PresetKind::Drums => {
            let every_other: Vec<(usize, u8)> = (0..STEPS).step_by(2).map(|i| (i, 1)).collect();
            vec![
                drums(
                    "4x4",
                    &[
                        ("kick", &[(0, 2), (4, 1), (8, 1), (12, 1)]),
                        ("clap", &[(4, 1), (12, 1)]),
                        ("chat", &[(2, 1), (6, 1), (10, 1), (14, 1)]),
                    ],
                    &[],
                ),
                drums(
                    "Breakbeat",
                    &[
                        ("kick", &[(0, 2), (6, 1), (10, 1)]),
                        ("snare", &[(4, 2), (12, 2), (7, 1)]),
                        ("chat", &every_other),
                        ("ohat", &[(14, 1)]),
                    ],
                    &[("snare_decay", 0.4)],
                ),
                drums(
                    "Minimal",
                    &[
                        ("kick", &[(0, 2), (8, 1)]),
                        ("clap", &[(12, 1)]),
                        ("tom", &[(11, 1)]),
                        ("chat", &[(2, 1), (6, 1), (10, 1), (14, 1)]),
                    ],
                    &[("swing", 0.3), ("kick_decay", 0.6)],
                ),
            ]
        }
    }
}

// ---- storage ------------------------------------------------------------------------------

/// `slug`: keep `[A-Za-z0-9 _-]`, trim, then at most 40 characters (not trimmed again).
pub fn slug(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        .collect();
    kept.trim().chars().take(40).collect()
}

/// Python's `repr(float)`: shortest round-trip digits, exponent when the decimal
/// point falls outside -4..16.
fn py_float(x: f64) -> String {
    let e = format!("{x:e}");
    let (mant, exp) = e.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("integer exponent");
    let (sign, mant) = match mant.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mant),
    };
    let point = exp + 1;
    if !(-4 < point && point <= 16) {
        let s = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{mant}e{s}{:02}", exp.abs());
    }
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let body = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= n {
        format!("{digits}{}.0", "0".repeat((point - n) as usize))
    } else {
        let (a, b) = digits.split_at(point as usize);
        format!("{a}.{b}")
    };
    format!("{sign}{body}")
}

/// A JSON string with Python's `ensure_ascii` escaping.
fn py_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn write_value(v: &Value, level: usize, out: &mut String) {
    let pad = |out: &mut String, l: usize| {
        out.push('\n');
        out.push_str(&" ".repeat(l));
    };
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => out.push_str(&py_float(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => py_str(s, out),
        Value::Array(a) if a.is_empty() => out.push_str("[]"),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, level + 1);
                write_value(x, level + 1, out);
            }
            pad(out, level);
            out.push(']');
        }
        Value::Object(m) if m.is_empty() => out.push_str("{}"),
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, level + 1);
                py_str(k, out);
                out.push_str(": ");
                write_value(x, level + 1, out);
            }
            pad(out, level);
            out.push('}');
        }
    }
}

/// `json.dumps(v, indent=1)`, byte for byte (no trailing newline).
pub fn dumps(v: &Value) -> String {
    let mut out = String::new();
    write_value(v, 0, &mut out);
    out
}

/// Factory presets plus `<dir>/<kind>/<slug>.json`; `dir = None` means factory only.
pub struct PresetStore {
    dir: Option<PathBuf>,
}

impl PresetStore {
    pub fn new(dir: Option<PathBuf>) -> PresetStore {
        PresetStore { dir }
    }

    pub fn can_save(&self) -> bool {
        self.dir.is_some()
    }

    /// `*.json` in the kind's folder, sorted by name (like `sorted(glob)`).
    fn user_files(&self, kind: PresetKind) -> Vec<PathBuf> {
        let Some(d) = self.dir.as_ref().map(|d| d.join(kind.as_str())) else {
            return Vec::new();
        };
        let Ok(rd) = std::fs::read_dir(&d) else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".json"))
            })
            .collect();
        v.sort();
        v
    }

    /// A valid preset of `kind`, or `None` for anything wrong with the file.
    fn load(path: &Path, kind: PresetKind) -> Option<Value> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let data: Value = serde_json::from_str(&text).ok()?;
        check(kind, &data).ok()?;
        Some(data)
    }

    /// Factory names first, then the valid user presets in file order.
    pub fn list(&self, kind: PresetKind) -> Vec<(String, Origin)> {
        let mut names: Vec<(String, Origin)> = factory(kind)
            .iter()
            .map(|p| {
                (
                    p["name"].as_str().unwrap_or_default().to_string(),
                    Origin::Factory,
                )
            })
            .collect();
        for f in self.user_files(kind) {
            if let Some(name) = Self::load(&f, kind)
                .and_then(|d| d.get("name").and_then(Value::as_str).map(str::to_string))
            {
                names.push((name, Origin::User));
            }
        }
        names
    }

    pub fn get(&self, kind: PresetKind, name: &str) -> Result<Value, String> {
        if let Some(p) = factory(kind).into_iter().find(|p| p["name"] == name) {
            return Ok(p);
        }
        self.user_files(kind)
            .iter()
            .filter_map(|f| Self::load(f, kind))
            .find(|d| d.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| name.to_string())
    }

    /// Write a user preset; returns the stored (cleaned) name.
    pub fn save(&self, kind: PresetKind, name: &str, data: &Value) -> Result<String, String> {
        let clean = slug(name);
        if clean.is_empty() {
            return Err("nombre vacío".into());
        }
        let lower = clean.to_lowercase();
        if factory(kind).iter().any(|p| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.to_lowercase() == lower)
        }) {
            return Err("ese nombre es de un preset de fábrica".into());
        }
        let Some(dir) = &self.dir else {
            return Err("no hay carpeta de presets".into());
        };
        let mut data = data.clone();
        if let Some(o) = data.as_object_mut() {
            o.insert("name".into(), Value::from(clean.clone()));
        }
        check(kind, &data)?;
        let path = dir.join(kind.as_str()).join(format!("{clean}.json"));
        let tmp = dir.join(kind.as_str()).join(format!("{clean}.json.tmp"));
        let result = std::fs::create_dir_all(dir.join(kind.as_str()))
            .and_then(|()| std::fs::write(&tmp, dumps(&data)))
            .and_then(|()| std::fs::rename(&tmp, &path));
        let _ = std::fs::remove_file(&tmp);
        result.map(|()| clean).map_err(|e| e.to_string())
    }
}
