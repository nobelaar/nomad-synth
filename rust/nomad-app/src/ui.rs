//! The NOMAD window. `NomadApp::ui` draws everything (and runs without a window
//! in tests); state comes from the mirrors and goes out as commands.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use egui::{Panel, RichText};
use nomad_core::engine::Kind;
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::Telemetry;
use nomad_runtime::midi::input::MidiInput;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::output::AudioOutput;

use crate::model::AppModel;
use crate::theme::GREY;
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
}

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
        }
    }

    /// Draw one frame (header, transport bar, scope + master, panel area).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.model.tick();
        Panel::top("header").show(ui, |ui| crate::header::header(ui, self));
        Panel::top("transport").show(ui, |ui| crate::transport::transport(ui, self));
        Panel::right("side")
            .resizable(false)
            .exact_size(270.0)
            .show(ui, |ui| {
                let t = Arc::clone(&self.telemetry);
                scope(ui, &t.scope, &mut self.scope_ms, self.sample_rate);
                ui.separator();
                if knob(ui, &mut self.master, "MASTER", false).changed {
                    self.model.set_master(self.master.value());
                }
            });
        egui::CentralPanel::default().show(ui, |ui| {
            let kind = self
                .model
                .selected()
                .and_then(|id| self.model.view(id))
                .map(|v| v.kind);
            let text = match kind {
                Some(Kind::Synth) => "Panel del Synth (M3-B)",
                Some(Kind::Drums) => "Machine Drum (M3-C)",
                None => "",
            };
            ui.centered_and_justified(|ui| ui.label(RichText::new(text).color(GREY)));
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
