//! Machine Drum view (`DrumView` of `nomad/gui/drums_view.py`): SWING and CLEAR,
//! then one row per instrument (name = trigger, tune/decay/level knobs, 16 steps).
//! Values come from the mirror through `ParamSync`; a tune/decay change re-renders
//! the one-shot here (off the audio thread), throttled like `DrumMachine.set_value`.

use std::time::{Duration, Instant};

use egui::{Frame, RichText, Sense, Stroke, Vec2};
use nomad_core::clock::STEPS;
use nomad_core::drum_synth::render_one_shot;
use nomad_core::params::{DRUM_PARAMS, INSTRUMENTS};
use nomad_runtime::Command;
use nomad_runtime::mirror::DrumSnapshot;

use crate::help::{reg, reg_rect};
use crate::model::{AppModel, InstrumentView};
use crate::param_sync::SynthParams;
use crate::theme::{ACCENT, BG, GREEN, GREY, PLAYHEAD, TROUGH};
use crate::ui::WIDE;
use crate::widgets::knob::knob;
use crate::widgets::knob_model::KnobModel;

/// Minimum time between two one-shot renders (`REGEN_COOLDOWN_S` in `drums.py`).
pub const REGEN_COOLDOWN_S: f64 = 0.06;
const SWING: usize = 19;
const TRACKS: usize = INSTRUMENTS.len();

/// `_SHORT`: the knob label from the parameter name's last part.
fn short(p: usize) -> &'static str {
    match DRUM_PARAMS[p].name.rsplit('_').next() {
        Some("tune") => "tune",
        Some("decay") => "decay",
        Some("level") => "level",
        Some("bpm") => "BPM",
        _ => "SWING",
    }
}

/// Collects tune/decay changes: the first one after a quiet period renders at
/// once, later ones wait until `REGEN_COOLDOWN_S` after the last render.
#[derive(Default)]
pub struct Regen {
    dirty: [bool; TRACKS],
    last: Option<Instant>,
}

impl Regen {
    pub fn touch(&mut self, track: usize) {
        if track < TRACKS {
            self.dirty[track] = true;
        }
    }

    /// Tracks to render now (empty during the cooldown).
    pub fn due(&mut self, now: Instant) -> Vec<usize> {
        let cooldown = Duration::from_secs_f64(REGEN_COOLDOWN_S);
        let ready = self
            .last
            .is_none_or(|t| now.saturating_duration_since(t) >= cooldown);
        if !ready || !self.dirty.contains(&true) {
            return Vec::new();
        }
        self.last = Some(now);
        let due = (0..TRACKS).filter(|&t| self.dirty[t]).collect();
        self.dirty = [false; TRACKS];
        due
    }
}

pub struct DrumViewState {
    values: SynthParams,
    regen: Regen,
    context: Option<(usize, egui::Pos2)>,
}

impl Default for DrumViewState {
    fn default() -> Self {
        let defaults: Vec<f64> = DRUM_PARAMS.iter().map(|s| s.default).collect();
        DrumViewState {
            values: SynthParams::new(&defaults),
            regen: Regen::default(),
            context: None,
        }
    }
}

/// `toggle_step`: shift toggles 0 <-> 2, a plain click 0 <-> 1 (an accent goes off).
pub fn toggled(current: u8, accent: bool) -> u8 {
    match (accent, current) {
        (true, 2) => 0,
        (true, _) => 2,
        (false, 0) => 1,
        (false, _) => 0,
    }
}

struct Ctx<'a> {
    state: &'a mut DrumViewState,
    inst: &'a InstrumentView,
    snap: DrumSnapshot,
    send: Vec<Command>,
    now: Instant,
}

impl Ctx<'_> {
    fn set(&mut self, p: usize, v: f64) {
        self.state.values.user_set(p, v, self.now);
        if let Some(s) = self.state.values.get_mut(p) {
            s.mark_sent();
        }
        self.send.push(Command::DrumValue {
            id: self.inst.id,
            param: p as u16,
            value: v,
        });
        if p < 3 * TRACKS && p % 3 < 2 {
            self.state.regen.touch(p / 3);
        }
    }

    fn knob(&mut self, ui: &mut egui::Ui, p: usize) {
        let v = self.state.values.ui_value(p, self.snap.values[p], self.now);
        let mut m = KnobModel::new(&DRUM_PARAMS[p], v);
        let r = knob(ui, &mut m, short(p), false);
        reg_rect(ui.ctx(), r.response.interact_rect, DRUM_PARAMS[p].name);
        if r.changed {
            self.set(p, m.value());
        }
        if r.context {
            let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
            self.state.context = Some((p, pos));
        }
    }

    fn steps(&mut self, ui: &mut egui::Ui, t: usize, playhead: i32) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for s in 0..STEPS {
                if s > 0 && s % 4 == 0 {
                    ui.add_space(4.0);
                }
                let level = self.snap.steps[t][s];
                let (rect, r) = ui.allocate_exact_size(Vec2::new(22.0, 20.0), Sense::click());
                let r = reg(r, "drum_steps");
                let name = format!("{} {}", INSTRUMENTS[t], s + 1);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                let fill = match level {
                    1 => GREEN,
                    2 => ACCENT,
                    _ => TROUGH,
                };
                let border = if playhead == s as i32 { PLAYHEAD } else { BG };
                ui.painter().rect_filled(rect, 0.0, fill);
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, border),
                    egui::StrokeKind::Inside,
                );
                if r.clicked() {
                    let accent = ui.input(|i| i.modifiers.shift);
                    self.send.push(Command::DrumSetStep {
                        id: self.inst.id,
                        track: t as u8,
                        step: s as u8,
                        level: toggled(level, accent),
                    });
                }
            }
        });
    }

    fn row(&mut self, ui: &mut egui::Ui, t: usize, wide: bool, playhead: i32) {
        let head = |c: &mut Self, ui: &mut egui::Ui| {
            let name = RichText::new(INSTRUMENTS[t].to_uppercase()).color(GREY);
            let b = ui.add(
                egui::Button::new(name)
                    .fill(TROUGH)
                    .min_size(Vec2::new(52.0, 20.0)),
            );
            if reg(b, &format!("drum_{}", INSTRUMENTS[t])).clicked() {
                c.send.push(Command::DrumTrigger {
                    id: c.inst.id,
                    track: t as u8,
                    velocity: 1.0,
                });
            }
            for f in 0..3 {
                c.knob(ui, 3 * t + f);
            }
        };
        if wide {
            ui.horizontal(|ui| {
                head(self, ui);
                ui.add_space(6.0);
                self.steps(ui, t, playhead);
            });
        } else {
            ui.horizontal(|ui| head(self, ui));
            self.steps(ui, t, playhead);
        }
    }

    fn context_menu(&mut self, ui: &egui::Ui) {
        let Some((p, pos)) = self.state.context else {
            return;
        };
        let mut close = false;
        let area = egui::Area::new(ui.scope_id().with(("drum-menu", self.inst.id)))
            .fixed_pos(pos)
            .order(egui::Order::Foreground);
        let r = area.show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(DRUM_PARAMS[p].name).color(GREEN));
                if ui.button("Valor por defecto").clicked() {
                    self.set(p, DRUM_PARAMS[p].default);
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

/// The whole drum panel of one machine; `now` drives the regen throttle.
pub fn drums_view_ui(
    ui: &mut egui::Ui,
    state: &mut DrumViewState,
    inst: &InstrumentView,
    model: &mut AppModel,
    now: Instant,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut snap = DrumSnapshot::default();
    inst.shared.drum_snapshot(&mut snap);
    let playhead = inst.shared.step.load(Relaxed);
    let wide = ui.available_width() >= WIDE;
    let mut c = Ctx {
        state,
        inst,
        snap,
        send: Vec::new(),
        now,
    };
    let panel = ui.scope(|ui| {
        let top = ui.horizontal(|ui| {
            c.knob(ui, SWING);
            ui.add_space(8.0);
            if ui.add(egui::Button::new("CLEAR").fill(TROUGH)).clicked() {
                c.send.push(Command::DrumClear(inst.id));
            }
        });
        reg_rect(ui.ctx(), top.response.rect, "drum_transport");
        for t in 0..TRACKS {
            c.row(ui, t, wide, playhead);
        }
    });
    reg_rect(ui.ctx(), panel.response.rect, "drums_view");
    c.context_menu(ui);
    // Values first, then the shots rendered from them (the local values: the mirror lags).
    let sr = model.sample_rate();
    for t in c.state.regen.due(now) {
        let v = |p: usize| DRUM_PARAMS[p].clamp(c.state.values.get(p).map_or(0.0, |s| s.value()));
        let shot = render_one_shot(t, sr, v(3 * t), v(3 * t + 1));
        c.send.push(Command::DrumSetBank {
            id: inst.id,
            track: t as u8,
            shot: Box::new(shot),
        });
    }
    for cmd in c.send {
        model.send(cmd);
    }
}
