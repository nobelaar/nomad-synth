//! MCP tools: descriptors and the UI-thread dispatcher (grows task by task).

use std::time::Instant;

use nomad_core::engine::InstrumentId;
use serde_json::Value;

use crate::ui::NomadApp;

/// A note-off owed to `target` (the synth that got the note-on) at `due`.
pub struct PendingOff {
    pub due: Instant,
    pub note: u8,
    pub target: Option<InstrumentId>,
}

pub fn descriptors() -> Vec<Value> {
    Vec::new()
}

pub fn call(
    _app: &mut NomadApp,
    _offs: &mut Vec<PendingOff>,
    name: &str,
    _args: &Value,
) -> Result<Value, String> {
    Err(format!("unknown tool: {name}"))
}
