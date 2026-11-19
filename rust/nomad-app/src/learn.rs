//! MIDI learn from the UI (`_toggle_learn`, `_arm`, `_learn_finished`, `_unbind`
//! of `nomad/gui/app.py`). The map is the one `MidiInput` shares: arming it here
//! makes the next CC on the MIDI thread bind; `poll` notices that and saves.

use std::sync::Mutex;

use nomad_core::params::SYNTH_PARAMS;
use nomad_runtime::midi::map::MidiMap;

/// What a control's context menu asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LearnAction {
    Arm(Option<usize>),
    Unbind(usize),
}

#[derive(Default, Debug)]
pub struct LearnState {
    /// Last synth parameter the user moved (for the LEARN button).
    pub last_touched: Option<usize>,
    /// Parameter this UI armed and is waiting for.
    pub armed_by_us: Option<usize>,
}

fn lock(m: &Mutex<MidiMap>) -> std::sync::MutexGuard<'_, MidiMap> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn name(pid: usize) -> &'static str {
    SYNTH_PARAMS.get(pid).map_or("?", |s| s.name)
}

/// Parameters that have a CC bound.
pub fn bound_params(map: &Mutex<MidiMap>) -> Vec<usize> {
    lock(map)
        .bindings()
        .iter()
        .filter_map(|(_, p)| SYNTH_PARAMS.iter().position(|s| s.name == p))
        .collect()
}

impl LearnState {
    pub fn touch(&mut self, pid: usize) {
        self.last_touched = Some(pid);
    }

    /// The LEARN button: cancel if armed, else arm the last touched control.
    pub fn toggle_learn(&mut self, map: &Mutex<MidiMap>) -> String {
        if self.armed_by_us.is_some() {
            self.arm(map, None)
        } else if let Some(p) = self.last_touched {
            self.arm(map, Some(p))
        } else {
            "LEARN: touch a control first".into()
        }
    }

    pub fn arm(&mut self, map: &Mutex<MidiMap>, pid: Option<usize>) -> String {
        // Every SYNTH_PARAMS name is a valid learn target.
        let _ = lock(map).arm(pid.map(name));
        self.armed_by_us = pid;
        pid.map_or(String::new(), |p| format!("LEARN: {} — move a CC", name(p)))
    }

    /// Each frame: if our learn was consumed by a CC, save the map and report.
    pub fn poll(&mut self, map: &Mutex<MidiMap>) -> Option<String> {
        let p = self.armed_by_us?;
        let m = lock(map);
        if m.learning().is_some() {
            return None;
        }
        self.armed_by_us = None;
        Some(match m.save() {
            Ok(()) => format!("LEARN: {} bound", name(p)),
            Err(e) => format!("LEARN: could not save map ({e})"),
        })
    }

    pub fn unbind(&mut self, map: &Mutex<MidiMap>, pid: usize) -> String {
        let mut m = lock(map);
        m.unbind(name(pid));
        match m.save() {
            Ok(()) => format!("MIDI: {} desvinculado", name(pid)),
            Err(e) => format!("MIDI: could not save map ({e})"),
        }
    }

    /// Apply a context-menu action; returns the status text.
    pub fn apply(&mut self, map: &Mutex<MidiMap>, action: LearnAction) -> String {
        match action {
            LearnAction::Arm(p) => self.arm(map, p),
            LearnAction::Unbind(p) => self.unbind(map, p),
        }
    }
}
