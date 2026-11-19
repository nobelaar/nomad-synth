use egui::{Event, Modifiers, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::keyboard::*;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::theme;
use nomad_core::engine::Engine;
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;

#[test]
fn note_at_matches_python() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/keyboard.json");
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let mut n = 0;
    for c in cases.as_array().unwrap() {
        let (oct, base) = (
            c["octaves"].as_u64().unwrap() as usize,
            c["base"].as_u64().unwrap() as u8,
        );
        let (w, h) = (c["width"].as_f64().unwrap(), c["height"].as_f64().unwrap());
        let ys: Vec<Option<f64>> = c["ys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|y| y.as_f64())
            .collect();
        for (x, row) in c["xs"]
            .as_array()
            .unwrap()
            .iter()
            .zip(c["notes"].as_array().unwrap())
        {
            let x = x.as_f64().unwrap();
            for (y, want) in ys.iter().zip(row.as_array().unwrap()) {
                let got = note_at(x, *y, w, h, oct, base);
                assert_eq!(
                    got.map(u64::from),
                    want.as_u64(),
                    "x {x} y {y:?} {oct} {base} {w}"
                );
                n += 1;
            }
        }
    }
    assert!(n > 10000);
}

#[test]
fn drag_moves_note_off_then_on() {
    let mut k = KeyboardState::default();
    assert_eq!(k.press_or_move(Some(60)), (None, Some(60)));
    assert_eq!(k.press_or_move(Some(60)), (None, None));
    assert_eq!(k.press_or_move(Some(62)), (Some(60), Some(62)));
    assert_eq!(k.press_or_move(None), (Some(62), None));
}

#[test]
fn release_and_focus_loss_send_note_off() {
    let mut k = KeyboardState::default();
    k.press_or_move(Some(64));
    assert_eq!(k.release(), Some(64));
    assert_eq!(k.release(), None);
}

#[test]
fn black_keys_win_overlap() {
    // One octave, 350 wide: C# straddles the C/D boundary at x = 50.
    assert_eq!(note_at(48.0, Some(10.0), 350.0, 84.0, 1, 60), Some(61));
    assert_eq!(note_at(48.0, Some(70.0), 350.0, 84.0, 1, 60), Some(60));
    assert_eq!(note_at(48.0, None, 350.0, 84.0, 1, 60), Some(60));
}

struct S {
    model: AppModel,
    core: AudioCore,
    kb: KeyboardState,
}

fn render(core: &mut AudioCore, n: usize) -> f32 {
    let mut peak = 0.0f32;
    let mut out = vec![0.0f32; 512];
    for _ in 0..n {
        core.render_stereo(&mut out, 1);
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

fn harness() -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let model = AppModel::new(ctl, SR, views);
    let mut h = Harness::builder()
        .with_size(Vec2::new(1000.0, 120.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                let shared = s.model.view(1).unwrap().shared.clone();
                keyboard_ui(ui, &mut s.kb, &|n| shared.active_note(n), &mut s.model);
            },
            S {
                model,
                core,
                kb: KeyboardState::default(),
            },
        );
    h.run_steps(2);
    h
}

#[test]
fn ui_press_sends_note_on_and_release_note_off() {
    let mut h = harness();
    let r = h.get_by_label("teclado").rect();
    let pos = egui::pos2(r.min.x + 5.0, r.max.y - 5.0); // the first white key, low
    h.event(Event::PointerMoved(pos));
    h.step();
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
    assert_eq!(h.state().kb.held(), Some(BASE_NOTE));
    assert!(
        render(&mut h.state_mut().core, 10) > 0.01,
        "the target synth sounds"
    );
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
    assert_eq!(h.state().kb.held(), None);
    render(&mut h.state_mut().core, 120);
    assert_eq!(render(&mut h.state_mut().core, 4), 0.0, "released");
}

#[test]
fn lit_keys_come_from_mirror_active_notes() {
    let mut h = harness();
    h.state_mut().model.send(Command::NoteOn {
        note: 60,
        velocity: 0.8,
    });
    render(&mut h.state_mut().core, 1);
    h.run_steps(2);
    assert!(h.state().model.view(1).unwrap().shared.active_note(60));
}
