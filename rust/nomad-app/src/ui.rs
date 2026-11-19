//! The NOMAD window. `NomadApp::ui` draws everything (and runs without a window
//! in tests); state comes from the mirrors and goes out as commands.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::Panel;
use nomad_core::engine::{InstrumentId, Kind};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::Telemetry;
use nomad_runtime::midi::input::MidiInput;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::output::AudioOutput;

use crate::drums_view::{DrumViewState, drums_view_ui};
use crate::keyboard::{KeyboardState, keyboard_ui};
use crate::model::AppModel;
use crate::seq_view::seq_view_ui;
use crate::synth_panel::{SynthPanelState, synth_sections_ui, voice_ui};
use crate::widgets::knob::knob;
use crate::widgets::knob_model::KnobModel;
use crate::widgets::scope::scope;

pub struct NomadApp {
    pub model: AppModel,
    pub output: Option<AudioOutput>,
    pub midi: Option<Rc<MidiInput>>,
    pub midi_map: Arc<Mutex<MidiMap>>,
    pub telemetry: Arc<Telemetry>,
    pub status: String,
    pub audio_device: Option<usize>,
    pub sample_rate: f64,
    scope_ms: f64,
    master: KnobModel,
    closed: bool,
    synth_panels: HashMap<InstrumentId, SynthPanelState>,
    drum_panels: HashMap<InstrumentId, DrumViewState>,
    keyboards: HashMap<InstrumentId, KeyboardState>,
    rec_intent: HashMap<InstrumentId, bool>,
}

/// Below this width the right column moves under the panel.
pub const WIDE: f32 = 1000.0;

impl NomadApp {
    pub fn new(
        mut model: AppModel,
        output: Option<AudioOutput>,
        midi: Option<MidiInput>,
        midi_map: Arc<Mutex<MidiMap>>,
        telemetry: Arc<Telemetry>,
    ) -> NomadApp {
        let midi = midi.map(Rc::new);
        if let Some(m) = &midi {
            // MIDI follows the selected synth: release held keys before it moves.
            let m = Rc::clone(m);
            model.set_on_retarget(Box::new(move || m.reset()));
        }
        let spec = &SYNTH_PARAMS[pid::MASTER_VOLUME];
        NomadApp {
            model,
            output,
            midi,
            midi_map,
            telemetry,
            status: String::new(),
            audio_device: None,
            sample_rate: 48000.0,
            scope_ms: 10.0,
            master: KnobModel::new(spec, spec.default),
            closed: false,
            synth_panels: HashMap::new(),
            drum_panels: HashMap::new(),
            keyboards: HashMap::new(),
            rec_intent: HashMap::new(),
        }
    }

    /// Instances that currently have per-panel UI state (tests).
    pub fn panel_state_ids(&self) -> Vec<InstrumentId> {
        let mut v: Vec<_> = self.synth_panels.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// Scope, MASTER and (for a synth) VOICE: the right column, or under the panel when narrow.
    fn side(
        &mut self,
        ui: &mut egui::Ui,
        synth: Option<&crate::model::InstrumentView>,
        now: Instant,
    ) {
        let t = Arc::clone(&self.telemetry);
        scope(ui, &t.scope, &mut self.scope_ms, self.sample_rate);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if knob(ui, &mut self.master, "MASTER", false).changed {
                self.model.set_master(self.master.value());
            }
            if let Some(inst) = synth {
                let st = self.synth_panels.entry(inst.id).or_default();
                voice_ui(ui, st, inst, &mut self.model, now);
            }
        });
    }

    /// Draw one frame. Layout adapts to the width: the right column moves under
    /// the panel below `WIDE`, sections reflow (4 / 2 / 1 columns) and the
    /// center scrolls vertically.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.model.tick();
        let now = Instant::now();
        // Forget the UI state of closed tabs.
        let live: Vec<InstrumentId> = self.model.instruments().iter().map(|v| v.id).collect();
        self.synth_panels.retain(|id, _| live.contains(id));
        self.drum_panels.retain(|id, _| live.contains(id));
        self.keyboards.retain(|id, _| live.contains(id));
        self.rec_intent.retain(|id, _| live.contains(id));

        Panel::top("header").show(ui, |ui| crate::header::header(ui, self));
        Panel::top("transport").show(ui, |ui| crate::transport::transport(ui, self));
        let selected = self
            .model
            .selected()
            .and_then(|id| self.model.view(id))
            .cloned();
        let synth = selected.as_ref().filter(|v| v.kind == Kind::Synth).cloned();
        if let Some(inst) = &synth {
            Panel::bottom("keyboard").show(ui, |ui| {
                let kb = self.keyboards.entry(inst.id).or_default();
                let shared = Arc::clone(&inst.shared);
                keyboard_ui(ui, kb, &|n| shared.active_note(n), &mut self.model);
            });
        }
        let wide = ui.available_width() >= WIDE;
        if wide {
            Panel::right("side")
                .resizable(false)
                .exact_size(270.0)
                .show(ui, |ui| self.side(ui, synth.as_ref(), now));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match &synth {
                        Some(inst) => {
                            let rec = self.rec_intent.entry(inst.id).or_insert(false);
                            seq_view_ui(ui, inst, &mut self.model, rec);
                            ui.separator();
                            let st = self.synth_panels.entry(inst.id).or_default();
                            synth_sections_ui(ui, st, inst, &mut self.model, now);
                        }
                        None => {
                            if let Some(inst) = selected.as_ref() {
                                let st = self.drum_panels.entry(inst.id).or_default();
                                drums_view_ui(ui, st, inst, &mut self.model, now);
                            }
                        }
                    }
                    if !wide {
                        ui.separator();
                        self.side(ui, synth.as_ref(), now);
                    }
                });
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }

    /// Stop audio and MIDI and release held keys; safe to call twice.
    pub fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(m) = &self.midi {
            m.reset();
        }
        if let Some(o) = &mut self.output {
            o.stop();
        }
        self.midi = None;
        self.output = None;
    }
}

impl Drop for NomadApp {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl eframe::App for NomadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        NomadApp::ui(self, ui);
    }
}
