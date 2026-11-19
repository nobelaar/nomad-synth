//! Every NOMAD widget in one window, to look at them by hand:
//! `cargo run --example gallery -p nomad-app`.

use nomad_app::theme;
use nomad_app::widgets::knob::knob;
use nomad_app::widgets::knob_model::KnobModel;
use nomad_app::widgets::scope::scope;
use nomad_app::widgets::selector::selector;
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::ScopeRing;

struct Gallery {
    knobs: Vec<(KnobModel, &'static str, bool)>,
    wave: usize,
    ring: ScopeRing,
    phase: f32,
    time_ms: f64,
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A test tone feeds the scope.
        let tone: Vec<f32> = (0..800)
            .map(|i| {
                let p = self.phase + i as f32 * 220.0 / 48000.0;
                (p * std::f32::consts::TAU).sin() * 0.8
            })
            .collect();
        self.phase = (self.phase + 800.0 * 220.0 / 48000.0).fract();
        self.ring.write(&tone);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("NOMAD widgets");
            ui.horizontal(|ui| {
                for (m, label, locked) in &mut self.knobs {
                    knob(ui, m, label, *locked);
                }
            });
            ui.separator();
            if let Some(i) = selector(ui, &["SIN", "TRI", "SAW", "SQR"], self.wave) {
                self.wave = i;
            }
            ui.label(theme::sym::ALL.join("  "));
            ui.separator();
            scope(ui, &self.ring, &mut self.time_ms, 48000.0);
        });
        ui.ctx().request_repaint();
    }
}

fn main() -> eframe::Result {
    let p = |id| &SYNTH_PARAMS[id];
    let knobs = vec![
        (KnobModel::new(p(pid::FILT_CUTOFF), 1200.0), "CUTOFF", false),
        (KnobModel::new(p(pid::FILT_RESO), 0.3), "RESO", false),
        (KnobModel::new(p(pid::AMP_ATTACK), 5.0), "ATTACK", false),
        (KnobModel::new(p(pid::OSC1_COARSE), 0.0), "COARSE", false),
        (KnobModel::new(p(pid::FILT_ENV_AMT), 0.0), "LOCKED", true),
    ];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "NOMAD widgets",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(Gallery {
                knobs,
                wave: 2,
                ring: ScopeRing::new(96000),
                phase: 0.0,
                time_ms: 10.0,
            }))
        }),
    )
}
