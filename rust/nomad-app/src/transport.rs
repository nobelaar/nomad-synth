//! Transport bar of the selected instrument (`InstanceStrip`): PLAY/STOP, BPM
//! with TAP, CLOCK source, PRESET/SAVE (M3-C), MUTE and level.

use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

use egui::{Button, DragValue, RichText, Slider};

use crate::theme::{BG, GREEN, GREY, TROUGH, sym};
use crate::ui::NomadApp;

pub fn transport(ui: &mut egui::Ui, app: &mut NomadApp) {
    let Some(id) = app.model.selected() else {
        ui.label("Sin instrumento: + para agregar uno");
        return;
    };
    let Some(v) = app.model.view(id).cloned() else {
        return;
    };
    let following = v.source.is_some();
    let m = &v.shared;
    let running = m.eff_running.load(Relaxed);
    ui.horizontal_wrapped(|ui| {
        let play = RichText::new(if running { "STOP" } else { "PLAY" })
            .size(16.0)
            .color(if running { BG } else { GREEN });
        let fill = if running { GREEN } else { TROUGH };
        if ui
            .add_enabled(
                !following,
                Button::new(play)
                    .fill(fill)
                    .min_size(egui::vec2(70.0, 28.0)),
            )
            .clicked()
        {
            app.model.toggle_play(id);
        }
        let bpm_label = ui.label("BPM");
        let mut bpm = m.eff_bpm.get();
        let r = ui
            .add_enabled(
                !following,
                DragValue::new(&mut bpm)
                    .range(20.0..=300.0)
                    .speed(0.5)
                    .fixed_decimals(0),
            )
            .labelled_by(bpm_label.id);
        if r.changed() {
            app.model.set_bpm(id, bpm);
        }
        if ui.add_enabled(!following, Button::new("-")).clicked() {
            app.model.set_bpm(id, (bpm - 1.0).round());
        }
        if ui.add_enabled(!following, Button::new("+")).clicked() {
            app.model.set_bpm(id, (bpm + 1.0).round());
        }
        if ui.add_enabled(!following, Button::new("TAP")).clicked() {
            app.model.tap_bpm(id, Instant::now());
        }
        let src = v
            .source
            .and_then(|s| app.model.view(s))
            .map_or("Internal".to_string(), |s| s.name.clone());
        let clock: String = format!("CLOCK: {src}").chars().take(26).collect();
        if ui.button(clock).clicked() && !app.model.cycle_source(id) {
            app.status = "CLOCK: no se puede sincronizar con esa fuente".into();
        }
        ui.add_enabled(false, Button::new(format!("PRESET {}", sym::DROPDOWN)));
        ui.add_enabled(false, Button::new("SAVE"));
        let muted = app.model.muted(id);
        let mute = RichText::new("MUTE").color(if muted { BG } else { GREY });
        if ui
            .add(Button::new(mute).fill(if muted { GREEN } else { TROUGH }))
            .clicked()
        {
            app.model.toggle_mute(id);
        }
        let level_label = ui.label("LEVEL");
        let mut level = m.level.get();
        if ui
            .add(Slider::new(&mut level, 0.0..=1.0).show_value(false))
            .labelled_by(level_label.id)
            .changed()
        {
            app.model.set_level(id, level);
        }
    });
}
