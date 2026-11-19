use egui::{Event, Modifiers, PointerButton, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::seq_view::*;
use nomad_app::theme;
use nomad_core::engine::{Engine, InstrumentId};
use nomad_runtime::mirror::SeqSnapshot;
use nomad_runtime::{AudioCore, Command, MidiSender, create};

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;

fn reference() -> serde_json::Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/seq_view.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn snap_with(on: u8, note: u8, vel: u8, extra: usize, step: usize, length: u8) -> SeqSnapshot {
    let mut s = SeqSnapshot {
        length,
        ..SeqSnapshot::default()
    };
    s.extra = [[255; 5]; 16];
    s.on[step] = on;
    s.note[step] = note;
    s.vel[step] = vel;
    for (j, x) in s.extra[step].iter_mut().enumerate().take(extra) {
        *x = note.saturating_add(3 + j as u8);
    }
    s
}

#[test]
fn pure_logic_matches_python() {
    let r = reference();
    for (n, want) in r["note_name"].as_object().unwrap() {
        assert_eq!(note_name(n.parse().unwrap()), want.as_str().unwrap(), "{n}");
    }
    for c in r["steps"].as_array().unwrap() {
        let u = |k: &str| c[k].as_i64().unwrap();
        let (step, pos) = (u("step") as usize, u("pos") as i32);
        let snap = snap_with(
            u("on") as u8,
            u("note") as u8,
            u("vel") as u8,
            u("extra") as usize,
            step,
            u("length") as u8,
        );
        let (top, bottom) = step_label(&snap, step);
        assert_eq!(
            [top.as_str(), bottom.as_str()],
            [
                c["label"][0].as_str().unwrap(),
                c["label"][1].as_str().unwrap()
            ],
            "{c}"
        );
        let st = step_style(&snap, step, pos);
        let fill = match st.fill {
            Fill::Off => "off",
            Fill::On => "on",
            Fill::Accent => "accent",
        };
        assert_eq!(fill, c["style"]["fill"], "{c}");
        assert_eq!(st.dimmed, c["style"]["dimmed"].as_bool().unwrap(), "{c}");
        assert_eq!(
            st.playhead,
            c["style"]["playhead"].as_bool().unwrap(),
            "{c}"
        );
    }
    for (d, (want_d, want_n)) in DIVS.iter().zip(
        r["divs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x[0].as_f64().unwrap(), x[1].as_str().unwrap())),
    ) {
        assert_eq!((d.0, d.1), (want_d, want_n));
    }
    for x in r["next_division"].as_array().unwrap() {
        assert_eq!(
            next_division(x[0].as_f64().unwrap()),
            x[1].as_f64().unwrap(),
            "{x}"
        );
    }
    for x in r["next_length"].as_array().unwrap() {
        assert_eq!(
            u64::from(next_length(x[0].as_u64().unwrap() as u8)),
            x[1].as_u64().unwrap()
        );
    }
    for x in r["rec"].as_array().unwrap() {
        let (rec, playing) = (x[0].as_bool().unwrap(), x[1].as_bool().unwrap());
        assert_eq!(rec_label(rec, playing), x[2].as_str().unwrap());
        assert_eq!(rest_enabled(rec, playing), x[3].as_bool().unwrap());
    }
}

#[test]
fn click_shift_click_rules() {
    let r = reference();
    for c in r["clicks"].as_array().unwrap() {
        let (on, vel) = (
            c["on"].as_u64().unwrap() as u8,
            c["vel"].as_u64().unwrap() as u8,
        );
        let snap = snap_with(on, 60, vel, 0, 2, 16);
        let Command::SeqSetStep {
            on: new_on,
            vel: new_vel,
            step,
            ..
        } = click_command(SYNTH, &snap, 2, c["accent"].as_bool().unwrap())
        else {
            panic!("not a SeqSetStep")
        };
        assert_eq!(step, 2);
        // A vel of None keeps the stored one (set_step semantics).
        let result = [u8::from(new_on), new_vel.unwrap_or(vel)];
        let want: Vec<u8> = c["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(result.to_vec(), want, "{c}");
    }
}

#[test]
fn wheel_transposes_whole_chord_via_command() {
    assert!(matches!(
        wheel_command(SYNTH, 4, 1, false),
        Command::SeqTranspose {
            step: 4,
            delta: 1,
            ..
        }
    ));
    assert!(matches!(
        wheel_command(SYNTH, 4, -2, false),
        Command::SeqTranspose { delta: -1, .. }
    ));
    assert!(matches!(
        wheel_command(SYNTH, 4, -1, true),
        Command::SeqTranspose { delta: -12, .. }
    ));
}

#[test]
fn rec_flow_updates_labels() {
    assert_eq!(rec_label(false, false), "REC");
    assert_eq!(
        (rec_label(true, false), rest_enabled(true, false)),
        ("REC·STEP", true)
    );
    assert_eq!(
        (rec_label(true, true), rest_enabled(true, true)),
        ("REC·LIVE", false)
    );
}

// ---- UI ----

struct S {
    model: AppModel,
    core: AudioCore,
    rec: bool,
    midi: Option<MidiSender>,
}

fn harness() -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, mut ctl) = create(e, SR);
    let midi = ctl.midi_sender();
    let model = AppModel::new(ctl, SR, views);
    let mut h = Harness::builder()
        .with_size(Vec2::new(1366.0, 300.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                let inst = s.model.view(SYNTH).cloned().unwrap();
                seq_view_ui(ui, &inst, &mut s.model, &mut s.rec);
            },
            S {
                model,
                core,
                rec: false,
                midi,
            },
        );
    step(&mut h);
    h
}

fn step(h: &mut Harness<'static, S>) {
    let mut out = vec![0.0f32; 512];
    h.state_mut().core.render_stereo(&mut out, 1);
    h.run_steps(3);
}

fn snap(h: &Harness<'static, S>) -> SeqSnapshot {
    let mut s = SeqSnapshot::default();
    h.state()
        .model
        .view(SYNTH)
        .unwrap()
        .shared
        .seq_snapshot(&mut s);
    s
}

/// Step buttons in order (their text starts with "·" or a note name and has two lines).
fn step_rects(h: &Harness<'static, S>) -> Vec<egui::Rect> {
    let mut r: Vec<egui::Rect> = h
        .query_all_by_label_contains("\n")
        .map(|n| n.rect())
        .collect();
    r.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    r
}

fn click_at(h: &mut Harness<'static, S>, pos: egui::Pos2, mods: Modifiers) {
    h.event(Event::ModifiersChanged(mods));
    h.event(Event::PointerMoved(pos));
    h.step();
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: mods,
    });
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: mods,
    });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

#[test]
fn ui_click_sends_seq_set_step() {
    let mut h = harness();
    let rects = step_rects(&h);
    assert_eq!(rects.len(), 16);
    click_at(&mut h, rects[3].center(), Modifiers::NONE);
    step(&mut h);
    let s = snap(&h);
    assert_eq!((s.on[3], s.vel[3]), (1, 100));
}

#[test]
fn ui_shift_click_accent() {
    let mut h = harness();
    let r = step_rects(&h)[5];
    click_at(&mut h, r.center(), Modifiers::SHIFT);
    step(&mut h);
    assert_eq!((snap(&h).on[5], snap(&h).vel[5]), (1, 127));
    click_at(&mut h, r.center(), Modifiers::SHIFT);
    step(&mut h);
    assert_eq!(snap(&h).on[5], 0, "shift-click on an accent turns it off");
}

#[test]
fn playhead_follows_mirror_step() {
    let mut h = harness();
    h.state_mut().model.send(Command::SetBpm {
        id: SYNTH,
        bpm: 120.0,
    });
    h.state_mut().model.send(Command::Start(SYNTH));
    for _ in 0..30 {
        step(&mut h); // 30 x 512 frames: past step 2 at 6000 frames per 16th
    }
    let m = h.state().model.view(SYNTH).unwrap().shared.clone();
    assert!(m.step.load(std::sync::atomic::Ordering::Relaxed) >= 2);
}

#[test]
fn rec_written_step_shows_chord_label() {
    let mut h = harness();
    h.get_by_label("REC").click();
    h.run_steps(2);
    step(&mut h);
    assert!(h.query_by_label("REC·STEP").is_some());
    assert!(h.query_by_label("REST").is_some());
    let midi = h.state_mut().midi.as_mut().unwrap();
    for n in [48u8, 52, 55] {
        midi.send(Command::NoteOn {
            note: n,
            velocity: 0.8,
        });
    }
    for n in [48u8, 52, 55] {
        midi.send(Command::NoteOff { note: n });
    }
    step(&mut h);
    assert!(h.query_by_label("C3\n+2").is_some());
}

#[test]
fn gate_slider_sends_gate() {
    let mut h = harness();
    let r = h.get_by_label("GATE").rect();
    // The slider sits right after its label: click near its right end.
    let slider = h
        .query_all_by_label("GATE")
        .map(|n| n.rect())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap();
    let pos = egui::pos2(slider.max.x - 2.0, slider.center().y);
    click_at(&mut h, pos, Modifiers::NONE);
    step(&mut h);
    assert!(snap(&h).gate > 0.9, "{} ({r:?})", snap(&h).gate);
}

#[test]
fn div_len_buttons_cycle() {
    let mut h = harness();
    h.get_by_label("DIV 1/16").click();
    h.run_steps(2);
    step(&mut h);
    assert_eq!(snap(&h).division, 0.5);
    assert!(h.query_by_label("DIV 1/8").is_some());
    h.get_by_label("LEN 16").click();
    h.run_steps(2);
    step(&mut h);
    assert_eq!(snap(&h).length, 1);
}
