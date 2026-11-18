//! MIDI learn: CC -> parameter bindings, persisted as the same JSON file as
//! Python (`nomad/midi/mapping.py`, the specification).
//!
//! One CC key `(port, channel, cc)` maps to one parameter and a parameter has at
//! most one CC (learning it again drops its old CC). The exact key wins;
//! otherwise the same `(channel, cc)` on any other port matches. Bindings keep
//! the insertion order of a Python dict: assigning an existing key keeps its
//! place, a new key goes last.

use std::io;
use std::path::{Path, PathBuf};

use nomad_core::params::SYNTH_PARAMS;

use super::parse::{MidiEvent, MidiKind};

pub const VERSION: i64 = 1;
const DISCRETE_SUFFIXES: [&str; 5] = ["_wave", "_mode", "_sync", "_src", "_dst"];

pub type Key = (String, u8, u8);

#[derive(Debug, PartialEq)]
pub struct UnknownParam(pub String);

fn param_id(name: &str) -> Option<usize> {
    SYNTH_PARAMS.iter().position(|s| s.name == name)
}

fn is_discrete(name: &str) -> bool {
    name == "polyphony" || DISCRETE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// `XDG_CONFIG_HOME/nomad-synth`, or `~/.config/nomad-synth` (Python's `default_config_dir`).
pub fn default_config_dir() -> PathBuf {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => PathBuf::from(x),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
    };
    base.join("nomad-synth")
}

pub struct MidiMap {
    /// Set by `load` when the file was unusable (`"<path>: <reason>"`).
    pub error: Option<String>,
    path: Option<PathBuf>,
    bindings: Vec<(Key, String)>,
    armed: Option<String>,
}

/// Python dict assignment: replace in place if the key exists, else append.
fn assign(b: &mut Vec<(Key, String)>, key: Key, param: String) {
    match b.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = param,
        None => b.push((key, param)),
    }
}

impl MidiMap {
    pub fn new(path: Option<PathBuf>) -> MidiMap {
        MidiMap {
            error: None,
            path,
            bindings: Vec::new(),
            armed: None,
        }
    }

    pub fn bindings(&self) -> &[(Key, String)] {
        &self.bindings
    }

    pub fn learning(&self) -> Option<&str> {
        self.armed.as_deref()
    }

    /// Bind the next CC to `param`; `None` cancels. An unknown name is an error.
    pub fn arm(&mut self, param: Option<&str>) -> Result<(), UnknownParam> {
        if let Some(p) = param
            && param_id(p).is_none()
        {
            return Err(UnknownParam(p.to_string()));
        }
        self.armed = param.map(String::from);
        Ok(())
    }

    pub fn unbind(&mut self, param: &str) {
        self.bindings.retain(|(_, v)| v != param);
    }

    /// A CC on `port`: learn it if armed, else look it up. Returns the parameter
    /// id and its value (`from_norm(value/127)`, discrete ones rounded and clamped).
    pub fn handle(&mut self, port: &str, e: &MidiEvent) -> Option<(usize, f64)> {
        if e.kind != MidiKind::Cc {
            return None; // a non-CC event does not consume the armed learn
        }
        let key = (port.to_string(), e.channel, e.cc);
        let param = match self.armed.take() {
            Some(armed) => {
                self.unbind(&armed);
                assign(&mut self.bindings, key, armed.clone());
                armed
            }
            None => {
                let exact = self.bindings.iter().find(|(k, _)| *k == key);
                let any = || {
                    self.bindings
                        .iter()
                        .find(|((_, ch, cc), _)| (*ch, *cc) == (e.channel, e.cc))
                };
                exact.or_else(any)?.1.clone()
            }
        };
        let id = param_id(&param)?;
        let spec = &SYNTH_PARAMS[id];
        let mut value = spec.from_norm(f64::from(e.value) / 127.0);
        if is_discrete(&param) {
            value = value.round_ties_even().max(spec.lo).min(spec.hi);
        }
        Some((id, value))
    }

    /// Write `{"version": 1, "bindings": [...]}` like Python's `json.dumps(indent=2)`,
    /// atomically (temporary file + rename). A failure keeps the previous file.
    /// Without a path it does nothing.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".tmp");
        let tmp = path.with_file_name(tmp_name);
        let result =
            std::fs::write(&tmp, self.to_json()).and_then(|()| std::fs::rename(&tmp, path));
        let _ = std::fs::remove_file(&tmp);
        result
    }

    fn to_json(&self) -> String {
        if self.bindings.is_empty() {
            return "{\n  \"version\": 1,\n  \"bindings\": []\n}".to_string();
        }
        let entries: Vec<String> = self
            .bindings
            .iter()
            .map(|((port, ch, cc), param)| {
                format!(
                    "    {{\n      \"port\": {},\n      \"channel\": {ch},\n      \"cc\": {cc},\n      \"param\": {}\n    }}",
                    json_str(port),
                    json_str(param)
                )
            })
            .collect();
        format!(
            "{{\n  \"version\": {VERSION},\n  \"bindings\": [\n{}\n  ]\n}}",
            entries.join(",\n")
        )
    }

    /// Replace the bindings from disk; never fails. A missing file gives an empty
    /// map; an unusable one an empty map plus `error`, without touching the file.
    /// Invalid entries are dropped.
    pub fn load(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.error = None;
        self.bindings.clear();
        if !path.exists() {
            return;
        }
        match read_entries(&path) {
            Ok(entries) => {
                for e in &entries {
                    if let Some((key, param)) = entry(e) {
                        assign(&mut self.bindings, key, param);
                    }
                }
            }
            Err(reason) => self.error = Some(format!("{}: {reason}", path.display())),
        }
    }
}

fn read_entries(path: &Path) -> Result<Vec<serde_json::Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let data: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let version = data.get("version").ok_or("missing 'version'")?;
    if version.as_f64() != Some(VERSION as f64) {
        return Err(format!("unsupported map version {version}"));
    }
    match data.get("bindings").ok_or("missing 'bindings'")? {
        serde_json::Value::Array(a) => Ok(a.clone()),
        _ => Err("bindings is not a list".into()),
    }
}

/// One valid binding, or `None` (Python: port str, channel 0..15 int, cc 0..127
/// int, known param; booleans and floats are not ints).
fn entry(e: &serde_json::Value) -> Option<(Key, String)> {
    let int = |k: &str, hi: u64| e.get(k)?.as_u64().filter(|&v| v <= hi).map(|v| v as u8);
    let port = e.get("port")?.as_str()?;
    let param = e.get("param")?.as_str()?;
    param_id(param)?;
    Some((
        (port.to_string(), int("channel", 15)?, int("cc", 127)?),
        param.to_string(),
    ))
}

/// JSON string like Python's `json.dumps` (ASCII only: non-ASCII as `\uXXXX`).
fn json_str(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap_or_default();
    let mut out = String::with_capacity(quoted.len());
    for c in quoted.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for u in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}
