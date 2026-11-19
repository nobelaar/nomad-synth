//! Header: title, one tab per instrument (`App.refresh_tabs`), `+` menu, audio
//! and MIDI menus, voices/xruns counters and the status line.

use std::sync::atomic::Ordering::Relaxed;

use egui::{Button, RichText};
use nomad_core::engine::Kind;
use nomad_runtime::midi::input::list_input_ports;
use nomad_runtime::output::list_output_devices;

use crate::theme::{BG, GREEN, GREY, TROUGH, sym};
use crate::ui::NomadApp;

pub fn header(ui: &mut egui::Ui, app: &mut NomadApp) {
    // Wraps on narrow (tiled) windows so VOICES/XRUNS never get cut.
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("NOMAD SYNTH").heading().color(GREEN));
        ui.separator();
        let ids: Vec<_> = app.model.instruments().iter().map(|v| v.id).collect();
        let (mut select, mut close) = (None, None);
        for id in ids {
            let Some(v) = app.model.view(id) else {
                continue;
            };
            let mut text = v.name.clone();
            if app.model.instruments().iter().any(|o| o.source == Some(id)) {
                text.push(' ');
                text.push_str(sym::MASTER);
            }
            if let Some(src) = v.source.and_then(|s| app.model.view(s)) {
                text.push_str(&format!(" {} {}", sym::FOLLOWS, src.name));
            }
            let active = app.model.selected() == Some(id);
            let fill = if active { GREEN } else { TROUGH };
            let fg = if active { BG } else { GREY };
            if ui
                .add(Button::new(RichText::new(text).color(fg)).fill(fill))
                .clicked()
            {
                select = Some(id);
            }
            if ui
                .add(Button::new(RichText::new(sym::CLOSE).color(GREY)).fill(TROUGH))
                .clicked()
            {
                close = Some(id);
            }
        }
        if let Some(id) = select {
            app.model.select(id);
        }
        if let Some(id) = close {
            app.model.close_instrument(id);
        }
        ui.menu_button("+", |ui| {
            for (label, kind) in [("Synth", Kind::Synth), ("Machine Drum", Kind::Drums)] {
                if ui.button(label).clicked() {
                    if app.model.add_instrument(kind).is_none() {
                        app.status = "No hay lugar para más instrumentos".into();
                    }
                    ui.close();
                }
            }
        });
        ui.separator();
        ui.menu_button(format!("AUDIO {}", sym::DROPDOWN), |ui| {
            let devices = list_output_devices();
            if devices.is_empty() {
                ui.label("(sin salidas)");
            }
            for d in devices {
                let current = app.audio_device == Some(d.index);
                if ui.selectable_label(current, &d.name).clicked() {
                    app.audio_device = Some(d.index);
                    app.status = "AUDIO: el cambio de dispositivo se aplica al reiniciar".into();
                    ui.close();
                }
            }
        });
        ui.menu_button(format!("MIDI {}", sym::DROPDOWN), |ui| {
            let ports = list_input_ports();
            if ports.is_empty() {
                ui.label("(sin puertos)");
            }
            let open = app.midi.as_ref().map(|m| m.port_name().to_string());
            for p in ports {
                if ui
                    .selectable_label(open.as_deref() == Some(p.as_str()), &p)
                    .clicked()
                {
                    app.status = "MIDI: el cambio de puerto se aplica al reiniciar".into();
                    ui.close();
                }
            }
        });
        let t = &app.telemetry;
        ui.label(format!("VOICES {}", t.voices_total.load(Relaxed)));
        ui.label(format!("XRUNS {}", t.xruns.load(Relaxed)));
        let armed = app.learn.armed_by_us.is_some();
        let learn = RichText::new("LEARN").color(if armed { BG } else { GREY });
        if ui
            .add(Button::new(learn).fill(if armed { GREEN } else { TROUGH }))
            .clicked()
        {
            app.status = app.learn.toggle_learn(&app.midi_map);
        }
        ui.add_enabled(false, Button::new("?"));
    });
    ui.label(RichText::new(&app.status).small());
}
