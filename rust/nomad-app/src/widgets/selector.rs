//! Row of flat buttons, the active one green on black (`Selector` of Python).

use egui::{Button, RichText};

use crate::theme::{BG, GREEN, GREY, TROUGH};

/// The clicked index, only on a click.
pub fn selector(ui: &mut egui::Ui, labels: &[&str], index: usize) -> Option<usize> {
    selector_with_context(ui, labels, index).0
}

/// Like `selector`, plus whether any button got a right or middle click.
pub fn selector_with_context(
    ui: &mut egui::Ui,
    labels: &[&str],
    index: usize,
) -> (Option<usize>, bool) {
    let (mut picked, mut context) = (None, false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (i, l) in labels.iter().enumerate() {
            let active = i == index;
            let text = RichText::new(*l).color(if active { BG } else { GREY });
            let r = ui.add(
                Button::new(text)
                    .fill(if active { GREEN } else { TROUGH })
                    .corner_radius(0.0),
            );
            if r.clicked() {
                picked = Some(i);
            }
            context |= r.secondary_clicked() || r.middle_clicked();
        }
    });
    (picked, context)
}
