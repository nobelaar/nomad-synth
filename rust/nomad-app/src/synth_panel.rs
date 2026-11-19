//! One synth's parameter controls (`SynthPanel` of `nomad/gui/synth_panel.py`):
//! the eight sections, the collapsible modulation matrix and the VOICE column.
//! Values come from the mirror through `ParamSync`; changes go out as
//! `Command::SetParam`.

use std::time::Instant;

use egui::{Frame, RichText, Stroke};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::Command;

use crate::learn::LearnAction;
use crate::model::{AppModel, InstrumentView};
use crate::param_sync::SynthParams;
use crate::synth_layout::{
    SECTIONS, Section, WidgetKind, selector_labels, selector_offset, short_label, widget_kind,
};
use crate::theme::{GREEN, TROUGH, sym};
use crate::widgets::knob::knob_hl;
use crate::widgets::knob_model::KnobModel;
use crate::widgets::selector::selector_with_context;

/// Grid order of the main sections (Python's `_steps`).
const MAIN: [&str; 8] = [
    "osc1", "osc2", "filter", "mixer", "amp_env", "filt_env", "lfo1", "lfo2",
];

pub struct SynthPanelState {
    pub matrix_open: bool,
    params: SynthParams,
    /// Parameter whose context menu is open, and where.
    context: Option<(usize, egui::Pos2)>,
    /// Set by the caller each frame: the parameter armed for MIDI learn...
    pub armed: Option<usize>,
    /// ...and the parameters that have a CC.
    pub bound: Vec<usize>,
    /// Out: the last parameter the user moved this frame.
    pub touched: Option<usize>,
    /// Out: a learn action picked in the context menu.
    pub learn: Option<LearnAction>,
}

impl Default for SynthPanelState {
    fn default() -> Self {
        let defaults: Vec<f64> = SYNTH_PARAMS.iter().map(|s| s.default).collect();
        SynthPanelState {
            matrix_open: false,
            params: SynthParams::new(&defaults),
            context: None,
            armed: None,
            bound: Vec::new(),
            touched: None,
            learn: None,
        }
    }
}

fn section(key: &str) -> &'static Section {
    SECTIONS
        .iter()
        .find(|s| s.key == key)
        .expect("known section")
}

/// Columns for the section grid at this width (4 / 2 / 1).
pub fn columns_for(width: f32) -> usize {
    if width >= 1000.0 {
        4
    } else if width >= 640.0 {
        2
    } else {
        1
    }
}

struct Ctx<'a> {
    state: &'a mut SynthPanelState,
    inst: &'a InstrumentView,
    model: &'a mut AppModel,
    now: Instant,
}

impl Ctx<'_> {
    fn value(&mut self, p: usize) -> f64 {
        let mirror = self.inst.shared.param(p);
        self.state.params.ui_value(p, mirror, self.now)
    }

    fn set(&mut self, p: usize, v: f64) {
        self.state.touched = Some(p);
        self.state.params.user_set(p, v, self.now);
        if self.model.send(Command::SetParam {
            id: self.inst.id,
            param: p as u16,
            value: v,
        }) && let Some(s) = self.state.params.get_mut(p)
        {
            s.mark_sent();
        }
    }

    /// Knob or selector for one parameter, with its context menu.
    fn control(&mut self, ui: &mut egui::Ui, p: usize) {
        let v = self.value(p);
        match widget_kind(p) {
            WidgetKind::Selector => {
                let off = selector_offset(p);
                let index = (v.round() as i64 - off).max(0) as usize;
                let (picked, context) = selector_with_context(ui, selector_labels(p), index);
                if let Some(i) = picked {
                    self.set(p, (i as i64 + off) as f64);
                }
                if context {
                    self.open_context(ui, p);
                }
            }
            WidgetKind::Knob => {
                let mut m = KnobModel::new(&SYNTH_PARAMS[p], v);
                let armed = self.state.armed == Some(p);
                let r = knob_hl(ui, &mut m, short_label(p), false, armed);
                if r.changed {
                    self.set(p, m.value());
                }
                if r.context {
                    self.open_context(ui, p);
                }
            }
        }
    }

    fn open_context(&mut self, ui: &egui::Ui, p: usize) {
        let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
        self.state.context = Some((p, pos));
    }

    fn section_ui(&mut self, ui: &mut egui::Ui, s: &Section, title: &str) {
        Frame::new()
            .stroke(Stroke::new(1.0, TROUGH))
            .inner_margin(3.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).color(GREEN));
                    for &p in s
                        .params
                        .iter()
                        .filter(|&&p| widget_kind(p) == WidgetKind::Selector)
                    {
                        self.control(ui, p);
                    }
                    ui.horizontal(|ui| {
                        for &p in s
                            .params
                            .iter()
                            .filter(|&&p| widget_kind(p) == WidgetKind::Knob)
                        {
                            self.control(ui, p);
                        }
                    });
                });
            });
    }

    fn matrix_ui(&mut self, ui: &mut egui::Ui, columns: usize) {
        let active = (0..8)
            .filter(|&i| {
                let base = pid::MOD0_SRC + 3 * i;
                [0, 1, 2]
                    .iter()
                    .all(|k| self.inst.shared.param(base + k) != 0.0)
            })
            .count();
        let arrow = if self.state.matrix_open {
            sym::DROPDOWN
        } else {
            sym::EXPAND
        };
        let text = format!("MODULACIÓN {arrow}  ({active} en uso)");
        if ui
            .add(egui::Button::new(RichText::new(text).color(GREEN)).fill(TROUGH))
            .clicked()
        {
            self.state.matrix_open = !self.state.matrix_open;
        }
        if !self.state.matrix_open {
            return;
        }
        let per_row = columns.clamp(1, 3);
        egui::Grid::new(("matrix", self.inst.id)).show(ui, |ui| {
            for i in 0..8 {
                let base = pid::MOD0_SRC + 3 * i;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.control(ui, base);
                        self.control(ui, base + 1);
                    });
                    self.control(ui, base + 2);
                });
                if i % per_row == per_row - 1 {
                    ui.end_row();
                }
            }
        });
    }

    fn context_menu(&mut self, ui: &egui::Ui) {
        let Some((p, pos)) = self.state.context else {
            return;
        };
        let mut close = false;
        let area = egui::Area::new(ui.scope_id().with(("param-menu", self.inst.id)))
            .fixed_pos(pos)
            .order(egui::Order::Foreground);
        let r = area.show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(SYNTH_PARAMS[p].name).color(GREEN));
                if ui.button("Valor por defecto").clicked() {
                    self.set(p, SYNTH_PARAMS[p].default);
                    close = true;
                }
                if self.state.armed == Some(p) {
                    if ui.button("Cancelar learn").clicked() {
                        self.state.learn = Some(LearnAction::Arm(None));
                        close = true;
                    }
                } else if ui.button("MIDI learn").clicked() {
                    self.state.learn = Some(LearnAction::Arm(Some(p)));
                    close = true;
                }
                if self.state.bound.contains(&p) && ui.button("Quitar MIDI").clicked() {
                    self.state.learn = Some(LearnAction::Unbind(p));
                    close = true;
                }
            });
        });
        let clicked_elsewhere =
            ui.input(|i| i.pointer.any_pressed()) && !r.response.contains_pointer();
        if close || (clicked_elsewhere && ui.input(|i| i.pointer.interact_pos()) != Some(pos)) {
            self.state.context = None;
        }
    }
}

/// The parameter sections and the modulation matrix of one synth.
pub fn synth_sections_ui(
    ui: &mut egui::Ui,
    state: &mut SynthPanelState,
    inst: &InstrumentView,
    model: &mut AppModel,
    now: Instant,
) {
    let columns = columns_for(ui.available_width());
    let mut c = Ctx {
        state,
        inst,
        model,
        now,
    };
    egui::Grid::new(("sections", inst.id))
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for (k, key) in MAIN.iter().enumerate() {
                let s = section(key);
                c.section_ui(ui, s, s.title);
                if k % columns == columns - 1 {
                    ui.end_row();
                }
            }
        });
    c.matrix_ui(ui, columns);
    c.context_menu(ui);
}

/// The VOICE section (polyphony, bend, glide) for the right column.
pub fn voice_ui(
    ui: &mut egui::Ui,
    state: &mut SynthPanelState,
    inst: &InstrumentView,
    model: &mut AppModel,
    now: Instant,
) {
    let mut c = Ctx {
        state,
        inst,
        model,
        now,
    };
    let s = section("global");
    c.section_ui(ui, s, s.title);
    // Its context menu is drawn by `synth_sections_ui` (one menu per panel state).
}
