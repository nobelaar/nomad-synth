//! On-screen keyboard (`Keyboard` of `nomad/gui/keyboard.py`): one note at a
//! time under the pointer, black keys win where they overlap, lit keys come from
//! the mirror. Notes go to the target synth as `NoteOn`/`NoteOff`.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use nomad_core::clock::py_floordiv;
use nomad_runtime::Command;

use crate::model::AppModel;
use crate::theme::{GREEN, TROUGH};

pub const WHITE_PC: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
/// White index in the octave -> black pitch class after it.
pub const BLACK_AFTER: [Option<u8>; 7] = [Some(1), Some(3), None, Some(6), Some(8), Some(10), None];
pub const BLACK_W: f64 = 0.6;
pub const BLACK_H: f64 = 0.6;
pub const VELOCITY: f32 = 0.8;
pub const BASE_NOTE: u8 = 36;
pub const OCTAVES: usize = 6;
pub const HEIGHT: f32 = 70.0;
const WHITE: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
const BLACK: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);

fn black_after(white_index: i64) -> Option<u8> {
    BLACK_AFTER[white_index.rem_euclid(7) as usize]
}

/// MIDI note under (x, y), or `None`; `y = None` means low on the key (white).
pub fn note_at(
    x: f64,
    y: Option<f64>,
    width: f64,
    height: f64,
    octaves: usize,
    base: u8,
) -> Option<u8> {
    if !(0.0..width).contains(&x) || y.is_some_and(|y| !(0.0..height).contains(&y)) {
        return None;
    }
    let ww = width / (7 * octaves) as f64;
    // Python's `x // ww` (fmod-based), not floor(x / ww): they differ on key edges.
    let wi = py_floordiv(x, ww) as i64;
    let note = |v: i64| u8::try_from(v).ok();
    if let Some(y) = y
        && y < BLACK_H * height
    {
        for j in [wi, wi + 1] {
            if let Some(pc) = black_after(j - 1)
                && 1 <= j
                && j < (7 * octaves) as i64
                && (x - j as f64 * ww).abs() < BLACK_W * ww / 2.0
            {
                return note(i64::from(base) + 12 * (j - 1).div_euclid(7) + i64::from(pc));
            }
        }
    }
    note(i64::from(base) + 12 * wi.div_euclid(7) + i64::from(WHITE_PC[wi.rem_euclid(7) as usize]))
}

#[derive(Default)]
pub struct KeyboardState {
    held: Option<u8>,
}

impl KeyboardState {
    pub fn held(&self) -> Option<u8> {
        self.held
    }

    /// Pointer down at `note`: returns (note to release, note to play).
    pub fn press_or_move(&mut self, note: Option<u8>) -> (Option<u8>, Option<u8>) {
        if note == self.held {
            return (None, None);
        }
        let off = self.held.take();
        self.held = note;
        (off, note)
    }

    pub fn release(&mut self) -> Option<u8> {
        self.held.take()
    }
}

/// (MIDI note, key rectangle) for each key of one color.
type Keys = Vec<(u8, Rect)>;

fn key_rects(rect: Rect, octaves: usize, base: u8) -> (Keys, Keys) {
    let (w, h) = (rect.width(), rect.height());
    let ww = w / (7 * octaves) as f32;
    let whites = (0..7 * octaves)
        .map(|i| {
            let n = base + 12 * (i / 7) as u8 + WHITE_PC[i % 7];
            (
                n,
                Rect::from_min_size(rect.min + Vec2::new(i as f32 * ww, 0.0), Vec2::new(ww, h)),
            )
        })
        .collect();
    let blacks = (1..7 * octaves)
        .filter_map(|j| {
            let pc = black_after(j as i64 - 1)?;
            let n = base + 12 * ((j - 1) / 7) as u8 + pc;
            let half = BLACK_W as f32 * ww / 2.0;
            let min = rect.min + Vec2::new(j as f32 * ww - half, 0.0);
            Some((
                n,
                Rect::from_min_size(min, Vec2::new(2.0 * half, BLACK_H as f32 * h)),
            ))
        })
        .collect();
    (whites, blacks)
}

/// Six octaves from `BASE_NOTE`, full available width, `HEIGHT` tall.
pub fn keyboard_ui(
    ui: &mut egui::Ui,
    state: &mut KeyboardState,
    lit: &dyn Fn(u8) -> bool,
    model: &mut AppModel,
) {
    let size = Vec2::new(ui.available_width().max(100.0), HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, "teclado"));
    let down = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let focused = ui.input(|i| i.focused);
    if down && focused {
        let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or(Pos2::ZERO);
        let local = pos - rect.min;
        let note = note_at(
            f64::from(local.x),
            Some(f64::from(local.y)),
            f64::from(rect.width()),
            f64::from(rect.height()),
            OCTAVES,
            BASE_NOTE,
        );
        let (off, on) = state.press_or_move(note);
        if let Some(n) = off {
            model.send(Command::NoteOff { note: n });
        }
        if let Some(n) = on {
            model.send(Command::NoteOn {
                note: n,
                velocity: VELOCITY,
            });
        }
    } else if let Some(n) = state.release() {
        // Button released, pointer left the area, or the window lost focus.
        model.send(Command::NoteOff { note: n });
    }
    let p = ui.painter_at(rect);
    let (whites, blacks) = key_rects(rect, OCTAVES, BASE_NOTE);
    let on = |n: u8| state.held() == Some(n) || lit(n);
    for (n, r) in whites {
        p.rect(
            r,
            0.0,
            if on(n) { GREEN } else { WHITE },
            Stroke::new(1.0, TROUGH),
            egui::StrokeKind::Inside,
        );
    }
    for (n, r) in blacks {
        p.rect(
            r,
            0.0,
            if on(n) { GREEN } else { BLACK },
            Stroke::new(1.0, TROUGH),
            egui::StrokeKind::Inside,
        );
    }
}
