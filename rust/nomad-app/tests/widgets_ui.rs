use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::theme;
use nomad_app::widgets::knob::knob;
use nomad_app::widgets::knob_model::{DRAG_PX, FINE, KnobModel, WHEEL_STEP};
use nomad_app::widgets::scope::scope;
use nomad_app::widgets::selector::selector;
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::ScopeRing;

struct KnobState {
    model: KnobModel,
    locked: bool,
    rect: Rect,
    changed: u32,
    resets: u32,
    contexts: u32,
}

fn knob_harness(locked: bool) -> Harness<'static, KnobState> {
    let spec = &SYNTH_PARAMS[pid::FILT_RESO]; // linear 0..1, default 0.3
    let state = KnobState {
        model: KnobModel::new(spec, 0.5),
        locked,
        rect: Rect::NOTHING,
        changed: 0,
        resets: 0,
        contexts: 0,
    };
    let mut h = Harness::new_ui_state(
        |ui, s: &mut KnobState| {
            let r = knob(ui, &mut s.model, "RESO", s.locked);
            s.rect = r.response.rect;
            s.changed += u32::from(r.changed);
            s.resets += u32::from(r.reset);
            s.contexts += u32::from(r.context);
        },
        state,
    );
    h.run();
    h
}

fn drag(h: &mut Harness<'static, KnobState>, dy: f32, modifiers: Modifiers) {
    let c = h.state().rect.center();
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(c));
    h.step();
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.step();
    // Small steps so egui's drag threshold is crossed and every pixel counts.
    let steps = 8;
    for k in 1..=steps {
        h.event(Event::PointerMoved(
            c + Vec2::new(0.0, dy * k as f32 / steps as f32),
        ));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: c + Vec2::new(0.0, dy),
        button: PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

#[test]
fn knob_drag_up_increases_value() {
    let mut h = knob_harness(false);
    drag(&mut h, -40.0, Modifiers::NONE);
    let n = h.state().model.norm();
    assert!((n - (0.5 + 40.0 / DRAG_PX)).abs() < 1e-6, "{n}");
    assert!(h.state().changed > 0);
}

#[test]
fn knob_shift_is_fine() {
    let mut h = knob_harness(false);
    drag(&mut h, -40.0, Modifiers::SHIFT);
    let n = h.state().model.norm();
    assert!((n - (0.5 + 40.0 / DRAG_PX / FINE)).abs() < 1e-6, "{n}");
}

#[test]
fn knob_wheel_steps() {
    let mut h = knob_harness(false);
    let c = h.state().rect.center();
    h.event(Event::PointerMoved(c));
    h.step();
    for _ in 0..3 {
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: Vec2::new(0.0, 1.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        h.step();
    }
    let n = h.state().model.norm();
    assert!((n - (0.5 + 3.0 * WHEEL_STEP)).abs() < 1e-9, "{n}");
}

#[test]
fn knob_double_click_resets_and_reports() {
    let mut h = knob_harness(false);
    let c = h.state().rect.center();
    // egui decides double clicks from event times: set them explicitly (kittest
    // does not advance RawInput.time between steps on its own).
    let mut t = 1.0;
    h.input_mut().time = Some(t);
    h.event(Event::PointerMoved(c));
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            t += 0.05;
            h.input_mut().time = Some(t);
            h.event(Event::PointerButton {
                pos: c,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    assert_eq!(h.state().resets, 1);
    assert!((h.state().model.value() - 0.3).abs() < 1e-12);
}

#[test]
fn knob_context_click_reports() {
    let mut h = knob_harness(false);
    let c = h.state().rect.center();
    h.event(Event::PointerMoved(c));
    h.step();
    for button in [PointerButton::Secondary, PointerButton::Middle] {
        h.event(Event::PointerButton {
            pos: c,
            button,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.event(Event::PointerButton {
            pos: c,
            button,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    assert_eq!(h.state().contexts, 2);
    assert_eq!(h.state().model.norm(), 0.5);
}

#[test]
fn locked_knob_ignores_input() {
    let mut h = knob_harness(true);
    drag(&mut h, -40.0, Modifiers::NONE);
    assert_eq!((h.state().model.norm(), h.state().changed), (0.5, 0));
}

#[test]
fn selector_click_returns_index_only_on_click() {
    let mut h = Harness::new_ui_state(
        |ui, s: &mut (usize, u32)| {
            if let Some(i) = selector(ui, &["SIN", "TRI", "SAW", "SQR"], s.0) {
                s.0 = i;
                s.1 += 1;
            }
        },
        (2usize, 0u32),
    );
    h.run();
    assert_eq!(*h.state(), (2, 0), "no click, no index");
    h.get_by_label("TRI").click();
    h.run();
    assert_eq!(*h.state(), (1, 1));
}

#[test]
fn scope_buttons_change_time() {
    let ring = ScopeRing::new(96000);
    ring.write(
        &(0..4800)
            .map(|i| (i as f32 * 0.05).sin())
            .collect::<Vec<_>>(),
    );
    let mut h = Harness::new_ui_state(move |ui, t: &mut f64| scope(ui, &ring, t, 48000.0), 10.0f64);
    h.run();
    h.get_by_label("50 ms").click();
    h.run();
    assert_eq!(*h.state(), 50.0);
}

#[test]
fn theme_sets_background() {
    let mut h = Harness::new_ui(|ui| {
        theme::apply(ui.ctx());
        ui.label("x");
    });
    h.run();
    assert_eq!(h.ctx.global_style().visuals.panel_fill, theme::BG);
    assert_eq!(theme::BG, egui::Color32::from_rgb(0x10, 0x13, 0x10));
}

#[test]
fn widgets_survive_scale_changes() {
    for ppp in [1.0, 1.5, 2.0] {
        let mut h = knob_harness(false);
        h.set_pixels_per_point(ppp);
        h.run();
        assert!(
            h.state().rect.width() > 10.0 && h.state().rect.height() > 10.0,
            "{ppp}"
        );
        drag(&mut h, -20.0, Modifiers::NONE);
        assert!(
            h.state().model.norm() > 0.5,
            "{ppp}: drag still hits the knob"
        );
    }
}

#[test]
fn default_font_has_the_design_symbols() {
    let h = Harness::new_ui(|ui| {
        ui.label("·");
    });
    let missing: Vec<char> = theme::sym::ALL
        .concat()
        .chars()
        .filter(|&c| {
            !h.ctx
                .fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(11.0), c))
        })
        .collect();
    assert!(missing.is_empty(), "missing glyphs: {missing:?}");
}
