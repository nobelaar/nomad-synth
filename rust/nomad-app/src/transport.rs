//! Transport bar of the selected instrument (`InstanceStrip`): PLAY/STOP, BPM
//! with TAP, CLOCK source, PRESET/SAVE, MUTE and level.

use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

use egui::{Button, DragValue, RichText, Slider};

use nomad_core::engine::InstrumentId;

use crate::help::reg;
use crate::model::InstrumentView;
use crate::presets::{Origin, PresetKind, apply_commands, capture};
use crate::theme::{BG, GREEN, GREY, TROUGH, sym};
use crate::ui::NomadApp;

/// Status line text, cut like Python's `[:80]`.
fn preset_error(e: &str) -> String {
    format!("PRESET: {e}").chars().take(80).collect()
}

fn button_text(name: &str) -> String {
    let short: String = name.chars().take(14).collect();
    format!("{short} {}", sym::DROPDOWN)
}

/// `load_preset`: apply the named preset to `v` and show its name on the button.
fn load_preset(app: &mut NomadApp, v: &InstrumentView, name: &str) {
    let kind = PresetKind::of(v.kind);
    let cmds = app
        .presets
        .get(kind, name)
        .map_err(|n| format!("'{n}'"))
        .and_then(|p| apply_commands(v, &p, app.model.sample_rate()));
    match cmds {
        Ok(cmds) => {
            for c in cmds {
                app.model.send(c);
            }
            app.preset_names.insert(v.id, name.to_string());
        }
        Err(e) => app.status = preset_error(&e),
    }
}

/// `save_preset`: open the name dialog (or report there is nowhere to save).
fn ask_save(app: &mut NomadApp, id: InstrumentId) {
    if !app.presets.can_save() {
        app.status = preset_error("no hay carpeta de presets");
        return;
    }
    app.save_dialog = Some((id, String::new()));
}

/// The modal "Guardar preset" dialog, while open.
pub fn save_dialog(ctx: &egui::Context, app: &mut NomadApp) {
    let Some((id, mut name)) = app.save_dialog.take() else {
        return;
    };
    let (mut save, mut cancel) = (false, false);
    egui::Modal::new(egui::Id::unique("save-preset")).show(ctx, |ui| {
        ui.label(RichText::new("Guardar preset").color(GREEN));
        let label = ui.label("Nombre:");
        let r = ui.text_edit_singleline(&mut name).labelled_by(label.id);
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        save = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            save |= ui.button("Guardar").clicked();
            cancel =
                ui.button("Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
    });
    if cancel {
        return;
    }
    if !save {
        app.save_dialog = Some((id, name));
        return;
    }
    if name.is_empty() {
        return; // Python: an empty answer just closes the dialog
    }
    let Some(v) = app.model.view(id).cloned() else {
        return;
    };
    match app
        .presets
        .save(PresetKind::of(v.kind), &name, &capture(&v, ""))
    {
        Ok(stored) => {
            app.status = format!("PRESET guardado: {stored}");
            app.preset_names.insert(id, stored);
        }
        Err(e) => app.status = preset_error(&e),
    }
}

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
        let b = ui.add_enabled(
            !following,
            Button::new(play)
                .fill(fill)
                .min_size(egui::vec2(70.0, 28.0)),
        );
        if reg(b, "inst_play").clicked() {
            app.model.toggle_play(id);
        }
        let bpm_label = reg(ui.label("BPM"), "inst_bpm");
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
        if reg(r, "inst_bpm").changed() {
            app.model.set_bpm(id, bpm);
        }
        let b = ui.add_enabled(!following, Button::new("-"));
        if reg(b, "inst_bpm").clicked() {
            app.model.set_bpm(id, (bpm - 1.0).round());
        }
        let b = ui.add_enabled(!following, Button::new("+"));
        if reg(b, "inst_bpm").clicked() {
            app.model.set_bpm(id, (bpm + 1.0).round());
        }
        let b = ui.add_enabled(!following, Button::new("TAP"));
        if reg(b, "inst_bpm").clicked() {
            app.model.tap_bpm(id, Instant::now());
        }
        let src = v
            .source
            .and_then(|s| app.model.view(s))
            .map_or("Internal".to_string(), |s| s.name.clone());
        let clock: String = format!("CLOCK: {src}").chars().take(26).collect();
        let b = ui.button(clock);
        if reg(b, "clock_source").clicked() && !app.model.cycle_source(id) {
            app.status = "CLOCK: no se puede sincronizar con esa fuente".into();
        }
        let text = button_text(app.preset_names.get(&id).map_or("PRESET", String::as_str));
        let menu = ui.menu_button(text, |ui| {
            // Listed when the menu opens, so presets saved since then show up.
            let kind = PresetKind::of(v.kind);
            let items = app
                .preset_items
                .get_or_insert_with(|| app.presets.list(kind));
            let mut picked = None;
            let mut last: Option<Origin> = None;
            for (name, origin) in items.iter() {
                if last.is_some_and(|l| l != *origin) {
                    ui.separator();
                }
                last = Some(*origin);
                if ui.button(name).clicked() {
                    picked = Some(name.clone());
                    ui.close();
                }
            }
            picked
        });
        reg(menu.response, "preset_menu");
        match menu.inner {
            Some(Some(name)) => load_preset(app, &v, &name),
            Some(None) => {}
            None => app.preset_items = None,
        }
        let b = ui.button("SAVE");
        if reg(b, "preset_save").clicked() {
            ask_save(app, id);
        }
        let muted = app.model.muted(id);
        let mute = RichText::new("MUTE").color(if muted { BG } else { GREY });
        let b = ui.add(Button::new(mute).fill(if muted { GREEN } else { TROUGH }));
        if reg(b, "inst_mute").clicked() {
            app.model.toggle_mute(id);
        }
        let level_label = reg(ui.label("LEVEL"), "inst_level");
        let mut level = m.level.get();
        let r = ui
            .add(Slider::new(&mut level, 0.0..=1.0).show_value(false))
            .labelled_by(level_label.id);
        if reg(r, "inst_level").changed() {
            app.model.set_level(id, level);
        }
    });
}
