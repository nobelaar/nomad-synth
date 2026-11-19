//! Step-sequencer editor (`SeqView` of `nomad/gui/seq_view.py`): pure rules
//! verified against Python, and the egui view that reads the mirror and sends
//! `Command::Seq*`.

use egui::{Button, RichText, Slider, Stroke};
use nomad_core::clock::STEPS;
use nomad_core::engine::InstrumentId;
use nomad_core::sequencer::NONE;
use nomad_runtime::Command;
use nomad_runtime::mirror::SeqSnapshot;

use crate::model::{AppModel, InstrumentView};
use crate::theme::{ACCENT, BG, GREEN, GREY, PLAYHEAD, TROUGH};
use crate::widgets::knob::wheel_notches;

pub const ACCENT_VEL: u8 = 127;
pub const NORMAL_VEL: u8 = 100;
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Step length in beats and its label.
pub const DIVS: [(f64, &str); 5] = [
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 bar"),
];

/// `C#4` (Python's floored `%` and `//` for any n).
pub fn note_name(n: i32) -> String {
    format!(
        "{}{}",
        NAMES[n.rem_euclid(12) as usize],
        n.div_euclid(12) - 1
    )
}

fn extras(snap: &SeqSnapshot, step: usize) -> usize {
    snap.extra[step].iter().filter(|&&n| n != NONE).count()
}

/// Two lines: the root note (or `·` when off) and `+N` extra chord notes (or a space).
pub fn step_label(snap: &SeqSnapshot, step: usize) -> (String, String) {
    let on = snap.on[step] != 0;
    let top = if on {
        note_name(i32::from(snap.note[step]))
    } else {
        "·".into()
    };
    let n = extras(snap, step);
    let bottom = if on && n > 0 {
        format!("+{n}")
    } else {
        " ".into()
    };
    (top, bottom)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    Off,
    On,
    Accent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepStyle {
    pub fill: Fill,
    /// Beyond LEN: shown off even if it is on.
    pub dimmed: bool,
    pub playhead: bool,
}

pub fn step_style(snap: &SeqSnapshot, step: usize, playhead: i32) -> StepStyle {
    let live = step < usize::from(snap.length);
    let on = snap.on[step] != 0;
    let fill = match (on && live, snap.vel[step] == ACCENT_VEL) {
        (false, _) => Fill::Off,
        (true, true) => Fill::Accent,
        (true, false) => Fill::On,
    };
    StepStyle {
        fill,
        dimmed: !live,
        playhead: playhead == step as i32,
    }
}

/// The division after the one closest to `cur`.
pub fn next_division(cur: f64) -> f64 {
    let i = (0..DIVS.len())
        .min_by(|&a, &b| (DIVS[a].0 - cur).abs().total_cmp(&(DIVS[b].0 - cur).abs()))
        .unwrap_or(0);
    DIVS[(i + 1) % DIVS.len()].0
}

pub fn division_label(d: f64) -> &'static str {
    DIVS.iter().find(|x| x.0 == d).map_or("?", |x| x.1)
}

pub fn next_length(cur: u8) -> u8 {
    cur % STEPS as u8 + 1
}

/// `toggle_step`: a plain click toggles (turning on sets velocity 100); an
/// accent click turns the step on with velocity 127, or off if it already was.
pub fn click_command(id: InstrumentId, snap: &SeqSnapshot, step: usize, accent: bool) -> Command {
    let (on, vel) = (snap.on[step] != 0, snap.vel[step]);
    let step = step as u8;
    if accent {
        Command::SeqSetStep {
            id,
            step,
            on: !(on && vel == ACCENT_VEL),
            note: None,
            vel: Some(ACCENT_VEL),
        }
    } else {
        Command::SeqSetStep {
            id,
            step,
            on: !on,
            note: None,
            vel: (!on).then_some(NORMAL_VEL),
        }
    }
}

/// Wheel over a step: transpose its chord a semitone per notch (an octave with shift).
pub fn wheel_command(id: InstrumentId, step: usize, notches: i32, octave: bool) -> Command {
    let delta = notches.signum() * if octave { 12 } else { 1 };
    Command::SeqTranspose {
        id,
        step: step as u8,
        delta: delta as i16,
    }
}

pub fn rec_label(rec: bool, playing: bool) -> &'static str {
    match (rec, playing) {
        (false, _) => "REC",
        (true, false) => "REC·STEP",
        (true, true) => "REC·LIVE",
    }
}

pub fn rest_enabled(rec: bool, playing: bool) -> bool {
    rec && !playing
}

fn flat(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    active: bool,
    fill_on: egui::Color32,
) -> egui::Response {
    let fill = if active { fill_on } else { TROUGH };
    ui.add(Button::new(RichText::new(text).color(if active { BG } else { GREY })).fill(fill))
}

/// Controls row and the 16 step buttons of one synth's sequencer.
pub fn seq_view_ui(
    ui: &mut egui::Ui,
    inst: &InstrumentView,
    model: &mut AppModel,
    rec_intent: &mut bool,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let id = inst.id;
    let mut snap = SeqSnapshot::default();
    inst.shared.seq_snapshot(&mut snap);
    let playing = inst.shared.eff_running.load(Relaxed);
    let rec = *rec_intent;
    let mut send = Vec::new();
    ui.horizontal_wrapped(|ui| {
        if flat(ui, "SEQ", snap.enabled, GREEN).clicked() {
            send.push(Command::SeqEnabled {
                id,
                enabled: !snap.enabled,
            });
        }
        if flat(ui, rec_label(rec, playing), rec, ACCENT).clicked() {
            *rec_intent = !rec;
            send.push(Command::SeqArm { id, on: !rec });
        }
        if ui
            .add_enabled(rest_enabled(rec, playing), Button::new("REST").fill(TROUGH))
            .clicked()
        {
            send.push(Command::SeqRest(id));
        }
        ui.add_space(12.0);
        if ui
            .button(format!("DIV {}", division_label(snap.division)))
            .clicked()
        {
            send.push(Command::SeqDivision {
                id,
                division: next_division(snap.division),
            });
        }
        if ui.button(format!("LEN {}", snap.length)).clicked() {
            send.push(Command::SeqLength {
                id,
                length: next_length(snap.length),
            });
        }
        ui.add_space(12.0);
        let gate_label = ui.label("GATE");
        let mut gate = (snap.gate * 100.0).round();
        let r = ui
            .add(Slider::new(&mut gate, 5.0..=100.0).show_value(false))
            .labelled_by(gate_label.id);
        if r.changed() {
            send.push(Command::SeqGate {
                id,
                gate: gate / 100.0,
            });
        }
        ui.add_space(12.0);
        if ui.button("CLEAR").clicked() {
            send.push(Command::SeqClear(id));
        }
    });
    // Playhead: the clock's step, or the REC cursor when the clock is stopped.
    let pos = if playing {
        inst.shared.step.load(Relaxed)
    } else if snap.rec {
        i32::from(snap.rec_step)
    } else {
        -1
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for s in 0..STEPS {
            if s > 0 && s % 4 == 0 {
                ui.add_space(8.0);
            }
            let (top, bottom) = step_label(&snap, s);
            let st = step_style(&snap, s, pos);
            let (fill, fg) = match st.fill {
                Fill::Off => (TROUGH, GREY),
                Fill::On => (GREEN, BG),
                Fill::Accent => (ACCENT, BG),
            };
            let stroke = if st.playhead {
                Stroke::new(3.0, PLAYHEAD)
            } else {
                Stroke::NONE
            };
            let b = Button::new(
                RichText::new(format!("{top}\n{bottom}"))
                    .color(fg)
                    .size(10.0),
            )
            .fill(fill)
            .stroke(stroke)
            .min_size(egui::vec2(40.0, 34.0));
            let r = ui.add(b);
            if r.clicked() {
                let accent = ui.input(|i| i.modifiers.shift);
                send.push(click_command(id, &snap, s, accent));
            }
            if r.hovered() {
                let n = wheel_notches(ui);
                if n != 0 {
                    send.push(wheel_command(id, s, n, ui.input(|i| i.modifiers.shift)));
                }
            }
        }
    });
    for c in send {
        model.send(c);
    }
}
