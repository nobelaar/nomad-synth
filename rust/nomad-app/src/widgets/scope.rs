//! Oscilloscope (`ScopeView` of Python): read the last 2n samples, find the
//! trigger in the first n and draw n from it; 5/10/20/50 ms buttons.

use egui::{Sense, Shape, Stroke, Vec2};
use nomad_runtime::ScopeRing;

use super::scope_math::{find_trigger, to_points};
use super::selector::selector;
use crate::theme::{GREEN, TROUGH};

pub const TIMES_MS: [f64; 4] = [5.0, 10.0, 20.0, 50.0];
const LABELS: [&str; 4] = ["5 ms", "10 ms", "20 ms", "50 ms"];
pub const HEIGHT: f32 = 126.0;

pub fn scope(ui: &mut egui::Ui, buffer: &ScopeRing, time_ms: &mut f64, sample_rate: f64) {
    let current = TIMES_MS.iter().position(|&t| t == *time_ms).unwrap_or(1);
    if let Some(i) = selector(ui, &LABELS, current) {
        *time_ms = TIMES_MS[i];
    }
    let width = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::hover());
    let n = ((*time_ms / 1000.0 * sample_rate) as usize).max(2);
    let mut window = vec![0.0f32; 2 * n];
    let got = buffer.window(2 * n, &mut window);
    let seg = if got < 2 * n {
        &window[..got.min(n)] // not full yet: from the start
    } else {
        let t = find_trigger(&window[..n]);
        &window[t..t + n]
    };
    let mut pts = Vec::new();
    to_points(seg, rect.width() as usize, rect.height() as usize, &mut pts);
    let p = ui.painter_at(rect);
    for k in 1..3 {
        let f = k as f32 / 3.0;
        p.line_segment(
            [
                rect.lerp_inside(Vec2::new(f, 0.0)),
                rect.lerp_inside(Vec2::new(f, 1.0)),
            ],
            Stroke::new(1.0, TROUGH),
        );
        p.line_segment(
            [
                rect.lerp_inside(Vec2::new(0.0, f)),
                rect.lerp_inside(Vec2::new(1.0, f)),
            ],
            Stroke::new(1.0, TROUGH),
        );
    }
    let line: Vec<_> = pts
        .iter()
        .map(|&(x, y)| rect.min + Vec2::new(x, y))
        .collect();
    p.add(Shape::line(line, Stroke::new(1.0, GREEN)));
}
