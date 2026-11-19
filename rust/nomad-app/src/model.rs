//! The UI-thread side of the instrument graph: `InstrumentManager` plus the
//! non-drawing part of `App` from `nomad/gui/app.py` and the strip actions of
//! `strip.py`. It owns the `ControlHandle` and the list of instruments, checks
//! the clock-source rules itself, and sends `Command`s; it reads state only from
//! the mirrors.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use nomad_core::engine::{Engine, InstrumentId, Kind, MAX_INSTANCES, build_instance};
use nomad_runtime::mirror::InstrumentShared;
use nomad_runtime::{Command, ControlHandle, Telemetry};

use crate::tap::TapTempo;

/// A closed instance keeps rendering its fade this long before it is dropped.
pub const CLOSE_DELAY: Duration = Duration::from_millis(100);
const MAX_VOICES: usize = 8;

#[derive(Clone)]
pub struct InstrumentView {
    pub id: InstrumentId,
    pub kind: Kind,
    pub name: String,
    pub shared: Arc<InstrumentShared>,
    /// Clock source as the UI decided it (`None` = Internal).
    pub source: Option<InstrumentId>,
}

/// Views of the instances an engine already has (call before `create`).
pub fn initial_views(e: &Engine) -> Vec<InstrumentView> {
    let mut out = Vec::new();
    e.instances_for_each(|i| {
        out.push(InstrumentView {
            id: i.id(),
            kind: i.kind(),
            name: i.name().to_string(),
            shared: Arc::clone(&i.shared),
            source: None,
        });
    });
    out
}

pub struct AppModel {
    control: ControlHandle,
    sample_rate: f64,
    instruments: Vec<InstrumentView>,
    selected: Option<InstrumentId>,
    target: Option<InstrumentId>,
    next_id: InstrumentId,
    counters: [usize; 2],
    muted: HashMap<InstrumentId, bool>,
    taps: HashMap<InstrumentId, TapTempo>,
    closing: Vec<Instant>,
    on_retarget: Option<Box<dyn FnMut()>>,
}

fn label(kind: Kind) -> (usize, &'static str) {
    match kind {
        Kind::Synth => (0, "Synth"),
        Kind::Drums => (1, "Machine Drum"),
    }
}

impl AppModel {
    pub fn new(control: ControlHandle, sample_rate: f64, initial: Vec<InstrumentView>) -> AppModel {
        let next_id = initial.iter().map(|v| v.id).max().unwrap_or(0) + 1;
        let mut counters = [0, 0];
        for v in &initial {
            counters[label(v.kind).0] += 1;
        }
        let target = initial.iter().find(|v| v.kind == Kind::Synth).map(|v| v.id);
        let selected = initial.first().map(|v| v.id);
        AppModel {
            control,
            sample_rate,
            instruments: initial,
            selected,
            target,
            next_id,
            counters,
            muted: HashMap::new(),
            taps: HashMap::new(),
            closing: Vec::new(),
            on_retarget: None,
        }
    }

    /// Called before the keyboard/MIDI target changes, so the caller can release
    /// held keys (`MidiInput::reset`) while they still reach the old synth.
    pub fn set_on_retarget(&mut self, f: Box<dyn FnMut()>) {
        self.on_retarget = Some(f);
    }

    pub fn instruments(&self) -> &[InstrumentView] {
        &self.instruments
    }

    pub fn view(&self, id: InstrumentId) -> Option<&InstrumentView> {
        self.instruments.iter().find(|v| v.id == id)
    }

    pub fn selected(&self) -> Option<InstrumentId> {
        self.selected
    }

    pub fn target_synth(&self) -> Option<InstrumentId> {
        self.target
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn telemetry(&self) -> Arc<Telemetry> {
        self.control.telemetry()
    }

    pub fn send(&mut self, cmd: Command) -> bool {
        self.control.send(cmd)
    }

    pub fn source_of(&self, id: InstrumentId) -> Option<InstrumentId> {
        self.view(id).and_then(|v| v.source)
    }

    fn followers(&self, id: InstrumentId) -> impl Iterator<Item = &InstrumentView> {
        self.instruments
            .iter()
            .filter(move |v| v.source == Some(id))
    }

    /// New instance with the next id and `"<Label> NN"`, built here (off the audio
    /// thread) and sent; it becomes the selection. `None` when the array is full.
    pub fn add_instrument(&mut self, kind: Kind) -> Option<InstrumentId> {
        if self.instruments.len() + self.closing.len() >= MAX_INSTANCES {
            return None;
        }
        let (k, label) = label(kind);
        let id = self.next_id;
        let name = format!("{label} {:02}", self.counters[k] + 1);
        let inst = build_instance(self.sample_rate, MAX_VOICES, kind, id, name.clone());
        let shared = Arc::clone(&inst.shared);
        if !self.control.send(Command::AddInstrument(inst)) {
            return None;
        }
        self.counters[k] += 1;
        self.next_id += 1;
        self.instruments.push(InstrumentView {
            id,
            kind,
            name,
            shared,
            source: None,
        });
        self.select(id);
        Some(id)
    }

    fn retarget(&mut self, id: Option<InstrumentId>) {
        if let Some(f) = &mut self.on_retarget {
            f();
        }
        self.target = id;
        if let Some(id) = id {
            self.control.send(Command::SetTargetSynth(id));
        }
    }

    /// Choose the instrument being edited; a synth also becomes the keyboard/MIDI target.
    pub fn select(&mut self, id: InstrumentId) {
        let Some(v) = self.view(id) else {
            return;
        };
        if v.kind == Kind::Synth && self.target != Some(id) {
            self.retarget(Some(id));
        }
        self.selected = Some(id);
    }

    pub fn close_instrument(&mut self, id: InstrumentId) {
        self.close_instrument_at(id, Instant::now());
    }

    /// `App.close_instance`: followers go Internal, the target moves to another
    /// synth if there is one, and the nearest tab becomes the selection.
    pub fn close_instrument_at(&mut self, id: InstrumentId, now: Instant) {
        let Some(idx) = self.instruments.iter().position(|v| v.id == id) else {
            return;
        };
        let was_target = self.target == Some(id);
        if was_target && let Some(f) = &mut self.on_retarget {
            f();
        }
        self.control.send(Command::RemoveInstrument(id));
        let original: Vec<InstrumentId> = self.instruments.iter().map(|v| v.id).collect();
        self.instruments.remove(idx);
        for v in &mut self.instruments {
            if v.source == Some(id) {
                v.source = None; // the engine hands the clock over the same way
            }
        }
        self.closing.push(now);
        self.muted.remove(&id);
        self.taps.remove(&id);
        if was_target {
            let alt = self
                .instruments
                .iter()
                .find(|v| v.kind == Kind::Synth)
                .map(|v| v.id);
            self.target = alt;
            if let Some(alt) = alt {
                self.control.send(Command::SetTargetSynth(alt));
            }
        }
        if self.selected == Some(id) {
            self.selected = None;
            let pos = |i: InstrumentId| original.iter().position(|&x| x == i).unwrap_or(0);
            let nearest = self
                .instruments
                .iter()
                .map(|v| v.id)
                .min_by_key(|&i| pos(i).abs_diff(idx));
            if let Some(n) = nearest {
                self.select(n);
            }
        }
    }

    /// Each frame: free what the audio thread handed back and drop closed instances
    /// after `CLOSE_DELAY`. Returns how many objects were reaped.
    pub fn tick(&mut self) -> usize {
        self.tick_at(Instant::now())
    }

    pub fn tick_at(&mut self, now: Instant) -> usize {
        let before = self.closing.len();
        self.closing
            .retain(|&t| now.duration_since(t) < CLOSE_DELAY);
        if self.closing.len() < before {
            self.control.send(Command::DropClosed);
        }
        self.control.reap()
    }

    /// `InstrumentManager.set_source`: false (and nothing sent) when the rules reject it.
    pub fn set_source(&mut self, id: InstrumentId, src: Option<InstrumentId>) -> bool {
        let Some(cur) = self.view(id).map(|v| v.source) else {
            return false;
        };
        if src == Some(id) {
            return false;
        }
        if let Some(s) = src {
            match self.view(s) {
                Some(sv) if sv.source.is_none() => {}
                _ => return false, // closed, or itself a follower: a chain
            }
            if self.followers(id).next().is_some() {
                return false; // others follow id: it cannot follow
            }
        }
        if src == cur {
            return true;
        }
        if let Some(v) = self.instruments.iter_mut().find(|v| v.id == id) {
            v.source = src;
        }
        self.control.send(Command::SetSource { id, src });
        true
    }

    /// Internal plus every other Internal instance (a master can only be Internal).
    pub fn source_options(&self, id: InstrumentId) -> Vec<Option<InstrumentId>> {
        if self.followers(id).next().is_some() {
            return vec![None];
        }
        std::iter::once(None)
            .chain(
                self.instruments
                    .iter()
                    .filter(|v| v.id != id && v.source.is_none())
                    .map(|v| Some(v.id)),
            )
            .collect()
    }

    /// `InstanceStrip.cycle_source`: the next option after the current one.
    pub fn cycle_source(&mut self, id: InstrumentId) -> bool {
        let opts = self.source_options(id);
        let cur = self.source_of(id);
        let next = opts
            .iter()
            .position(|&o| o == cur)
            .and_then(|i| opts[(i + 1) % opts.len()]);
        self.set_source(id, next)
    }

    fn own_clock(&self, id: InstrumentId) -> Option<&InstrumentView> {
        self.view(id).filter(|v| v.source.is_none())
    }

    /// PLAY/STOP; a follower's transport is its source's, so this is a no-op (`false`).
    pub fn toggle_play(&mut self, id: InstrumentId) -> bool {
        let Some(v) = self.own_clock(id) else {
            return false;
        };
        let running = v.shared.clock_running.load(Relaxed);
        self.control.send(if running {
            Command::Stop(id)
        } else {
            Command::Start(id)
        })
    }

    pub fn set_bpm(&mut self, id: InstrumentId, bpm: f64) -> bool {
        if self.own_clock(id).is_none() {
            return false;
        }
        self.control.send(Command::SetBpm { id, bpm })
    }

    pub fn tap_bpm(&mut self, id: InstrumentId, now: Instant) -> Option<f64> {
        let bpm = self.taps.entry(id).or_default().tap(now)?;
        self.set_bpm(id, bpm);
        Some(bpm)
    }

    /// The mute the user asked for (the mirror shows what the engine applied).
    pub fn muted(&self, id: InstrumentId) -> bool {
        self.muted.get(&id).copied().unwrap_or(false)
    }

    pub fn toggle_mute(&mut self, id: InstrumentId) {
        if self.view(id).is_none() {
            return;
        }
        let m = !self.muted(id);
        self.muted.insert(id, m);
        self.control.send(Command::SetMute { id, muted: m });
    }

    pub fn set_level(&mut self, id: InstrumentId, level: f64) {
        self.control.send(Command::SetLevel { id, level });
    }

    pub fn set_master(&mut self, v: f64) {
        self.control.send(Command::SetMaster(v));
    }
}
