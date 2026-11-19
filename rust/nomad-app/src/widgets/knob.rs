//! Rotary knob (`Knob` of `nomad/gui/knob.py`): 270° arc from the lower left,
//! clockwise; vertical drag (shift = fine), wheel, double click = default,
//! right/middle click = context (MIDI learn). It only reports: the caller sends.

use egui::{Align2, FontId, Pos2, Sense, Shape, Stroke, Vec2};

use super::knob_model::KnobModel;
use crate::theme::{GREEN, GREY, PLAYHEAD, TROUGH};

pub const W: f32 = 64.0;
pub const H: f32 = 84.0;
const R: f32 = 21.0;
const START_DEG: f32 = 225.0;
const SWEEP_DEG: f32 = -270.0;

pub struct KnobResponse {
    /// The value changed through a user gesture (drag, wheel or reset).
    pub changed: bool,
    pub reset: bool,
    /// Right or middle click.
    pub context: bool,
    pub hovered: bool,
    pub response: egui::Response,
}

fn arc_points(c: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = (((to - from).abs() / 6.0).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
            Pos2::new(c.x + r * a.cos(), c.y - r * a.sin())
        })
        .collect()
}

/// Wheel notches this frame (one per wheel event, its direction).
fn wheel_notches(ui: &egui::Ui) -> i32 {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => {
                    Some(delta.y.signum() as i32)
                }
                _ => None,
            })
            .sum()
    })
}

pub fn knob(ui: &mut egui::Ui, model: &mut KnobModel, label: &str, locked: bool) -> KnobResponse {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(W, H), Sense::click_and_drag());
    let enabled = !locked;
    let name = format!("{label} {}", model.format_value());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, enabled, &name));
    let hovered = response.hovered();
    let (mut changed, mut reset, mut context) = (false, false, false);
    if !locked {
        let fine = ui.input(|i| i.modifiers.shift);
        // Every pixel since the press counts (Tk's B1-Motion), not only those past
        // egui's drag threshold.
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) {
            let dy = ui.input(|i| i.pointer.delta().y);
            if dy != 0.0 {
                model.set_from_drag(f64::from(-dy), fine);
                changed = true;
            }
        }
        if hovered {
            let n = wheel_notches(ui);
            if n != 0 {
                model.set_from_wheel(n, fine);
                changed = true;
            }
        }
        if response.double_clicked() {
            model.reset();
            changed = true; // a reset always notifies
            reset = true;
        }
        context = response.secondary_clicked() || response.middle_clicked();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let small = FontId::proportional(10.0);
        p.text(
            Pos2::new(rect.center().x, rect.top() + 8.0),
            Align2::CENTER_CENTER,
            label,
            small.clone(),
            if hovered { GREEN } else { GREY },
        );
        let c = Pos2::new(rect.center().x, rect.top() + 14.0 + R + 4.0);
        let end = START_DEG + SWEEP_DEG;
        p.add(Shape::line(
            arc_points(c, R, START_DEG, end),
            Stroke::new(5.0, TROUGH),
        ));
        let now = START_DEG + SWEEP_DEG * model.norm() as f32;
        if model.norm() > 0.0 {
            p.add(Shape::line(
                arc_points(c, R, START_DEG, now),
                Stroke::new(5.0, GREEN),
            ));
        }
        let a = now.to_radians();
        let tip = Pos2::new(c.x + (R - 6.0) * a.cos(), c.y - (R - 6.0) * a.sin());
        p.line_segment(
            [c, tip],
            Stroke::new(2.0, if locked { GREY } else { PLAYHEAD }),
        );
        p.text(
            Pos2::new(rect.center().x, rect.bottom() - 9.0),
            Align2::CENTER_CENTER,
            model.format_value(),
            small,
            GREEN,
        );
    }
    KnobResponse {
        changed,
        reset,
        context,
        hovered,
        response,
    }
}
