use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::synth_layout::{SECTIONS, WidgetKind, short_label, widget_kind};
use nomad_app::synth_panel::{SynthPanelState, synth_sections_ui, voice_ui};
use nomad_app::theme;
use nomad_app::widgets::knob_model::KnobModel;
use nomad_core::engine::{Engine, InstrumentId, Kind};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;

struct S {
    model: AppModel,
    core: AudioCore,
    panels: Vec<(InstrumentId, SynthPanelState)>,
    shown: InstrumentId,
    now: Instant,
}

impl S {
    fn render(&mut self) {
        let mut out = vec![0.0f32; 512];
        self.core.render_stereo(&mut out, 1);
    }
    fn param(&self, id: InstrumentId, p: usize) -> f64 {
        self.model.view(id).unwrap().shared.param(p)
    }
}

fn harness() -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let model = AppModel::new(ctl, SR, views);
    let s = S {
        model,
        core,
        panels: vec![(1, SynthPanelState::default())],
        shown: 1,
        now: Instant::now(),
    };
    let mut h = Harness::builder()
        .with_size(Vec2::new(1366.0, 1400.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                let inst = s.model.view(s.shown).cloned().unwrap();
                let st = &mut s.panels.iter_mut().find(|(i, _)| *i == s.shown).unwrap().1;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    synth_sections_ui(ui, st, &inst, &mut s.model, s.now);
                    voice_ui(ui, st, &inst, &mut s.model, s.now);
                });
            },
            s,
        );
    h.state_mut().render();
    h.run_steps(3);
    h
}

fn step(h: &mut Harness<'static, S>) {
    h.state_mut().render();
    h.run_steps(3);
}

fn open_matrix(h: &mut Harness<'static, S>) {
    h.get_by_label_contains("MODULACIÓN").click();
    h.run_steps(3);
}

#[test]
fn all_visible_params_have_a_control() {
    let mut h = harness();
    open_matrix(&mut h);
    for s in SECTIONS {
        for &p in s.params {
            match widget_kind(p) {
                WidgetKind::Knob => {
                    assert!(
                        h.query_all_by_label_contains(short_label(p))
                            .next()
                            .is_some(),
                        "{} ({})",
                        SYNTH_PARAMS[p].name,
                        short_label(p)
                    );
                }
                WidgetKind::Selector => {}
            }
        }
    }
    // Selectors: 4 oscillator waves... count rows by their first labels.
    assert_eq!(
        h.query_all_by_label("SIN").count(),
        4,
        "osc1, osc2, lfo1, lfo2 waves"
    );
    assert_eq!(h.query_all_by_label("KEY").count(), 2);
    assert_eq!(
        h.query_all_by_label("OFF").count(),
        1,
        "osc2_sync only (osc1_sync hidden)"
    );
    assert_eq!(
        h.query_all_by_label("LFO1").count(),
        9,
        "8 matrix sources + the LFO1 section title"
    );
    assert_eq!(
        h.query_all_by_label("PIT").count(),
        8,
        "8 matrix destinations"
    );
    assert!(h.query_by_label("VOICE").is_some());
}

#[test]
fn knob_drag_sets_param_through_command() {
    let mut h = harness();
    let rect = h.get_by_label_contains("cutoff").rect();
    let c = rect.center();
    h.event(Event::PointerMoved(c));
    h.step();
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for k in 1..=8 {
        h.event(Event::PointerMoved(
            c + Vec2::new(0.0, -40.0 * k as f32 / 8.0),
        ));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: c - Vec2::new(0.0, 40.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    step(&mut h);
    let mut m = KnobModel::new(
        &SYNTH_PARAMS[pid::FILT_CUTOFF],
        SYNTH_PARAMS[pid::FILT_CUTOFF].default,
    );
    m.set_from_drag(40.0, false);
    let got = h.state().param(1, pid::FILT_CUTOFF);
    assert!(
        (got - m.value()).abs() < 1e-6 * m.value(),
        "{got} vs {}",
        m.value()
    );
}

#[test]
fn selector_click_sets_param_with_offset() {
    let mut h = harness();
    // Polyphony buttons "1".."8" exist only once (the VOICE section).
    h.get_by_label("4").click();
    h.run_steps(2);
    step(&mut h);
    assert_eq!(h.state().param(1, pid::POLYPHONY), 4.0);
}

#[test]
fn matrix_collapsed_by_default_and_counts_active() {
    let mut h = harness();
    assert!(h.query_all_by_label("PIT").next().is_none(), "collapsed");
    for (p, v) in [
        (pid::MOD0_SRC, 1.0),
        (pid::MOD0_DST, 5.0),
        (pid::MOD0_AMT, 0.3),
        (pid::MOD1_SRC, 2.0),
    ] {
        h.state_mut().model.send(Command::SetParam {
            id: 1,
            param: p as u16,
            value: v,
        });
    }
    step(&mut h);
    assert!(h.query_by_label_contains("(1 en uso)").is_some());
}

#[test]
fn matrix_toggle_shows_slots() {
    let mut h = harness();
    open_matrix(&mut h);
    assert!(h.query_all_by_label_contains("amt 3").next().is_some());
    assert!(
        h.query_by_label_contains(nomad_app::theme::sym::DROPDOWN)
            .is_some()
    );
    open_matrix(&mut h);
    assert!(h.query_all_by_label("PIT").next().is_none());
}

#[test]
fn context_default_resets_param() {
    let mut h = harness();
    h.state_mut().model.send(Command::SetParam {
        id: 1,
        param: pid::FILT_RESO as u16,
        value: 0.9,
    });
    step(&mut h);
    h.get_by_label_contains("reso").click_secondary();
    h.run_steps(3);
    h.get_by_label("Valor por defecto").click();
    h.run_steps(2);
    step(&mut h);
    assert_eq!(
        h.state().param(1, pid::FILT_RESO),
        SYNTH_PARAMS[pid::FILT_RESO].default
    );
    assert!(
        h.query_by_label("MIDI learn").is_some() || h.query_by_label("Valor por defecto").is_none()
    );
}

#[test]
fn each_synth_has_its_own_panel_state() {
    let mut h = harness();
    let s2 = h.state_mut().model.add_instrument(Kind::Synth).unwrap();
    h.state_mut().panels.push((s2, SynthPanelState::default()));
    step(&mut h);
    open_matrix(&mut h);
    assert_eq!(h.query_all_by_label("PIT").count(), 8);
    h.state_mut().shown = s2;
    h.run_steps(3);
    assert!(
        h.query_all_by_label("PIT").next().is_none(),
        "the second synth's matrix is still closed"
    );
}

#[test]
fn mirror_change_from_outside_updates_knob() {
    let mut h = harness();
    assert!(h.query_by_label("cutoff 1.20 kHz").is_some());
    h.state_mut().model.send(Command::SetParam {
        id: 1,
        param: pid::FILT_CUTOFF as u16,
        value: 5000.0,
    });
    step(&mut h);
    h.state_mut().now += Duration::from_millis(200);
    h.run_steps(3);
    assert!(h.query_by_label("cutoff 5.00 kHz").is_some());
}
