//! The NOMAD window. `NomadApp::ui` draws everything (and runs without a window
//! in tests); state comes from the mirrors and goes out as commands.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::Panel;
use nomad_core::engine::{InstrumentId, Kind};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::midi::input::MidiInput;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::output::AudioOutput;
use nomad_runtime::{AudioCore, MidiSender, Telemetry};

use crate::devices::{AudioOpener, OutputDevice, real_opener};
use crate::drums_view::{DrumViewState, drums_view_ui};
use crate::help::{HelpState, reg_rect};
use crate::keyboard::{KeyboardState, keyboard_ui};
use crate::learn::{LearnState, bound_params};
use crate::model::AppModel;
use crate::presets::{Origin, PresetStore};
use crate::seq_view::seq_view_ui;
use crate::spiral::{SpiralState, resolve_source, spiral_ui, sync_tap};
use crate::synth_panel::{SynthPanelState, synth_sections_ui, voice_ui};
use crate::widgets::knob::knob;
use crate::widgets::knob_model::KnobModel;
use crate::widgets::scope::scope;

pub struct NomadApp {
    pub model: AppModel,
    pub output: Option<Box<dyn OutputDevice>>,
    /// Opens the core on another device (`devices::real_opener`; tests inject a fake).
    pub opener: AudioOpener,
    /// The core while no device has it (startup or switch failed), for the next try.
    pub parked_core: Option<AudioCore>,
    /// Shared with the retarget hook, which releases held keys.
    pub midi: Rc<RefCell<Option<MidiInput>>>,
    /// The MIDI sender while no port is open, for the next port picked.
    pub parked_midi: Option<MidiSender>,
    pub midi_map: Arc<Mutex<MidiMap>>,
    pub telemetry: Arc<Telemetry>,
    pub status: String,
    pub audio_device: Option<usize>,
    pub sample_rate: f64,
    /// Factory only until the caller points it at `<config>/presets` (tests use a temp dir).
    pub presets: PresetStore,
    /// Name shown on each instrument's PRESET button.
    pub preset_names: HashMap<InstrumentId, String>,
    /// The PRESET menu's list while it is open.
    pub preset_items: Option<Vec<(String, Origin)>>,
    /// Open SAVE dialog: instrument and the name typed so far.
    pub save_dialog: Option<(InstrumentId, String)>,
    pub learn: LearnState,
    pub help: HelpState,
    /// The spiral tab, when open (one at most).
    pub spiral: Option<SpiralState>,
    /// The spiral tab is the one on screen.
    pub spiral_shown: bool,
    /// The MCP server bridge, when started.
    pub mcp: Option<crate::mcp::Mcp>,
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
        let midi = Rc::new(RefCell::new(midi));
        // MIDI follows the selected synth: release held keys before it moves.
        let hook = Rc::clone(&midi);
        model.set_on_retarget(Box::new(move || {
            if let Some(m) = hook.borrow().as_ref() {
                m.reset();
            }
        }));
        let spec = &SYNTH_PARAMS[pid::MASTER_VOLUME];
        NomadApp {
            model,
            output: output.map(|o| Box::new(o) as Box<dyn OutputDevice>),
            opener: real_opener(48000),
            parked_core: None,
            midi,
            parked_midi: None,
            midi_map,
            telemetry,
            status: String::new(),
            audio_device: None,
            sample_rate: 48000.0,
            presets: PresetStore::new(None),
            preset_names: HashMap::new(),
            preset_items: None,
            save_dialog: None,
            learn: LearnState::default(),
            help: HelpState::default(),
            spiral: None,
            spiral_shown: false,
            mcp: None,
            scope_ms: 10.0,
            master: KnobModel::new(spec, spec.default),
            closed: false,
            synth_panels: HashMap::new(),
            drum_panels: HashMap::new(),
            keyboards: HashMap::new(),
            rec_intent: HashMap::new(),
        }
    }

    /// The MASTER knob's value (the master volume lives only in the UI).
    pub fn master_value(&self) -> f64 {
        self.master.value()
    }

    /// Set the MASTER knob (clamped; non-finite is ignored) and send it to the engine.
    pub fn set_master(&mut self, v: f64) {
        if self.master.set_value(v).is_ok() {
            self.model.set_master(self.master.value());
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
        let sc = ui.scope(|ui| scope(ui, &t.scope, &mut self.scope_ms, self.sample_rate));
        reg_rect(ui.ctx(), sc.response.rect, "scope");
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let r = knob(ui, &mut self.master, "MASTER", false);
            reg_rect(ui.ctx(), r.response.interact_rect, "master_volume");
            if r.changed {
                self.model.set_master(self.master.value());
                self.learn.touch(pid::MASTER_VOLUME);
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
        crate::help::clear(ui.ctx());
        let (f1, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::F1),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if f1 || (esc && self.help.on) {
            self.toggle_help();
        }
        // Forget the UI state of closed tabs.
        let live: Vec<InstrumentId> = self.model.instruments().iter().map(|v| v.id).collect();
        self.synth_panels.retain(|id, _| live.contains(id));
        self.drum_panels.retain(|id, _| live.contains(id));
        self.preset_names.retain(|id, _| live.contains(id));
        self.keyboards.retain(|id, _| live.contains(id));
        self.rec_intent.retain(|id, _| live.contains(id));

        // Learn state the synth panels draw from; their requests are applied below.
        let bound = bound_params(&self.midi_map);
        for st in self.synth_panels.values_mut() {
            st.armed = self.learn.armed_by_us;
            st.bound.clone_from(&bound);
        }
        Panel::top("header").show(ui, |ui| crate::header::header(ui, self));
        Panel::top("transport").show(ui, |ui| crate::transport::transport(ui, self));
        crate::transport::save_dialog(&ui.ctx().clone(), self);
        let selected = self
            .model
            .selected()
            .and_then(|id| self.model.view(id))
            .cloned();
        if !self.spiral_shown
            && let Some(sp) = &mut self.spiral
        {
            sync_tap(sp, &mut self.model, None);
        }
        let spiral_on = self.spiral_shown && self.spiral.is_some();
        let synth = if spiral_on {
            let sp = self.spiral.as_ref().expect("spiral open");
            resolve_source(sp, &self.model)
                .and_then(|id| self.model.view(id))
                .cloned()
        } else {
            selected.as_ref().filter(|v| v.kind == Kind::Synth).cloned()
        };
        if let Some(inst) = &synth {
            Panel::bottom("keyboard").show(ui, |ui| {
                let kb = self.keyboards.entry(inst.id).or_default();
                let shared = Arc::clone(&inst.shared);
                let r =
                    ui.scope(|ui| keyboard_ui(ui, kb, &|n| shared.active_note(n), &mut self.model));
                reg_rect(ui.ctx(), r.response.rect, "keyboard");
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
                    if spiral_on && let Some(sp) = &mut self.spiral {
                        let t = Arc::clone(&self.telemetry);
                        spiral_ui(ui, sp, &mut self.model, &t.tap, self.sample_rate);
                    } else {
                        match &synth {
                            Some(inst) => {
                                let panel = ui.scope(|ui| {
                                    let rec = self.rec_intent.entry(inst.id).or_insert(false);
                                    seq_view_ui(ui, inst, &mut self.model, rec);
                                    ui.separator();
                                    let st = self.synth_panels.entry(inst.id).or_default();
                                    synth_sections_ui(ui, st, inst, &mut self.model, now);
                                });
                                reg_rect(ui.ctx(), panel.response.rect, "synth_view");
                            }
                            None => {
                                if let Some(inst) = selected.as_ref() {
                                    let st = self.drum_panels.entry(inst.id).or_default();
                                    drums_view_ui(ui, st, inst, &mut self.model, now);
                                }
                            }
                        }
                    }
                    if !wide {
                        ui.separator();
                        self.side(ui, synth.as_ref(), now);
                    }
                });
        });
        for st in self.synth_panels.values_mut() {
            if let Some(p) = st.touched.take() {
                self.learn.touch(p);
            }
            if let Some(a) = st.learn.take() {
                self.status = self.learn.apply(&self.midi_map, a);
            }
        }
        if let Some(msg) = self.learn.poll(&self.midi_map) {
            self.status = msg;
        }
        if self.help.on && crate::help::help_ui(ui.ctx(), &mut self.help, now) {
            self.toggle_help();
        }
        if let Some(mut m) = self.mcp.take() {
            m.drain(self);
            self.mcp = Some(m);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }

    /// Open the spiral tab (if not open yet) and show it.
    pub fn open_spiral(&mut self) {
        if self.spiral.is_none() {
            self.spiral = Some(SpiralState::new(self.sample_rate));
        }
        self.spiral_shown = true;
    }

    /// Close the spiral tab and stop the engine's tap.
    pub fn close_spiral(&mut self) {
        if let Some(mut s) = self.spiral.take() {
            sync_tap(&mut s, &mut self.model, None);
        }
        self.spiral_shown = false;
    }

    /// Help mode on/off (`?`, F1; Esc only turns it off).
    pub fn toggle_help(&mut self) {
        self.help.toggle();
        self.status = if self.help.on {
            crate::help::STATUS.to_string()
        } else {
            String::new()
        };
    }

    /// Stop audio and MIDI and release held keys; safe to call twice.
    pub fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(m) = self.midi.borrow_mut().take() {
            m.reset();
        }
        if let Some(o) = &mut self.output {
            o.stop();
        }
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
