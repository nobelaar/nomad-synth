//! Spiral tab: geometry, analysis and the tab's life cycle (open once, tap, close).

use std::sync::{Arc, Mutex};

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use nomad_app::help::{help_for, registered};
use nomad_app::model::{AppModel, initial_views};
use nomad_app::spiral::{
    HIGH, LOW, SpiralState, fft, harmonic_weight, hz_of, lowpass_gain, midi_of, note_label,
    spiral_pos,
};
use nomad_app::theme;
use nomad_app::ui::NomadApp;
use nomad_core::engine::Engine;
use nomad_core::oscillator::Wave;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, Command, ScopeRing, create};

const SR: f64 = 48000.0;

#[test]
fn every_c_points_up_and_octaves_grow_outwards() {
    let c = Pos2::new(100.0, 100.0);
    let mut last = 0.0;
    for o in 0..=8 {
        let p = spiral_pos(LOW + 12.0 * o as f64, c, 10.0, 90.0);
        assert!((p.x - c.x).abs() < 1e-3, "C straight up");
        assert!(p.y < c.y);
        let r = c.y - p.y;
        assert!(r > last);
        last = r;
    }
    assert!((c.y - spiral_pos(HIGH, c, 10.0, 90.0).y - 90.0).abs() < 1e-3);
    // F# (6 semitones) points straight down, E# = F right of centre-ish.
    let fs = spiral_pos(66.0, c, 10.0, 90.0);
    assert!((fs.x - c.x).abs() < 1e-3 && fs.y > c.y);
    let a = spiral_pos(57.0, c, 10.0, 90.0);
    assert!(a.x < c.x, "A is left of C (9 of 12, clockwise)");
}

#[test]
fn pitch_helpers() {
    assert!((midi_of(440.0) - 69.0).abs() < 1e-12);
    assert!((hz_of(60.0) - 261.6255653).abs() < 1e-6);
    assert_eq!(note_label(440.0), "A4");
    assert_eq!(note_label(hz_of(69.12)), "A4 +12¢");
    assert_eq!(note_label(hz_of(59.75)), "C4 -25¢");
    assert_eq!(harmonic_weight(Wave::Sine, 1), 1.0);
    assert_eq!(harmonic_weight(Wave::Sine, 2), 0.0);
    assert_eq!(harmonic_weight(Wave::Saw, 4), 0.25);
    assert_eq!(harmonic_weight(Wave::Square, 2), 0.0);
    assert_eq!(harmonic_weight(Wave::Triangle, 3), 1.0 / 9.0);
    assert!((lowpass_gain(1000.0, 1000.0) - 0.5f64.sqrt()).abs() < 1e-12);
    assert!(lowpass_gain(4000.0, 1000.0) < 0.004, "24 dB/oct");
}

#[test]
fn fft_finds_a_sine() {
    let n = 1024;
    let mut re: Vec<f32> = (0..n)
        .map(|i| (std::f32::consts::TAU * 37.0 * i as f32 / n as f32).cos())
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let mag = |k: usize| re[k].hypot(im[k]);
    assert!((mag(37) - n as f32 / 2.0).abs() < 0.1);
    assert!(mag(36) < 1e-2 && mag(100) < 1e-2);
}

#[test]
fn a_sine_lights_its_note_only() {
    let ring = ScopeRing::new(48000);
    let hz = hz_of(57.0); // A3
    let s: Vec<f32> = (0..16384)
        .map(|i| (0.5 * (std::f64::consts::TAU * hz * i as f64 / SR).sin()) as f32)
        .collect();
    ring.write(&s);
    let mut st = SpiralState::new(SR);
    st.analyse(&ring, SR);
    let lv = st.levels();
    let at = |m: f64| lv[((m - LOW) * 6.0).round() as usize];
    assert!(at(57.0) > 0.8, "{}", at(57.0));
    for m in [45.0, 52.0, 64.0, 69.0] {
        assert!(at(m) < 0.1, "{m}: {}", at(m));
    }
}

struct S {
    app: NomadApp,
    core: AudioCore,
}

fn harness() -> Harness<'static, S> {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let tel = ctl.telemetry();
    let model = AppModel::new(ctl, SR, views);
    let app = NomadApp::new(
        model,
        None,
        None,
        Arc::new(Mutex::new(MidiMap::new(None))),
        tel,
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1366.0, 900.0))
        .build_ui_state(
            |ui, s: &mut S| {
                theme::apply(ui.ctx());
                s.app.ui(ui);
            },
            S { app, core },
        );
    h.run_steps(3);
    h
}

fn step(h: &mut Harness<'static, S>) {
    let mut out = vec![0.0f32; 512];
    for _ in 0..20 {
        h.state_mut().core.render_stereo(&mut out, 1);
    }
    h.run_steps(3);
}

fn open(h: &mut Harness<'static, S>) {
    // Two "+": the add menu (header, on top) and the BPM step (transport).
    let add = h
        .get_all_by_label("+")
        .map(|n| n.rect())
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap()
        .center();
    h.hover_at(add);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: add,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.get_by_label("Espiral").click();
    h.run_steps(3);
}

#[test]
fn spiral_tab_opens_once_listens_and_closes() {
    let mut h = harness();
    open(&mut h);
    assert!(h.state().app.spiral_shown);
    assert_eq!(h.state().app.spiral.as_ref().unwrap().tapped, Some(1));
    // Played note: its exact name is drawn, on the synth being listened to.
    h.state_mut().app.model.send(Command::NoteOn {
        note: 57,
        velocity: 1.0,
    });
    step(&mut h);
    step(&mut h);
    // The tapped sound reaches the spiral: A3 (220 Hz) lights up, A2 below stays dark.
    let lv = h.state().app.spiral.as_ref().unwrap().levels().to_vec();
    let at = |m: f64| lv[((m - LOW) * 6.0).round() as usize];
    assert!(at(57.0) > 0.5, "A3: {}", at(57.0));
    assert!(at(45.0) < 0.1, "A2: {}", at(45.0));
    let keys: Vec<String> = registered(&h.ctx).into_iter().map(|(_, k)| k).collect();
    for k in ["tab_spiral", "spiral_source", "spiral_view"] {
        assert!(keys.iter().any(|x| x == k), "{k} registered");
        assert!(help_for(k).is_some(), "{k} has help");
    }
    // A second open keeps the one tab (the menu item is also disabled).
    h.state_mut().app.open_spiral();
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("Espiral").count(), 1);
    // Back to an instrument tab: the tap stops, the spiral stays open.
    h.get_by_label("Machine Drum 01").click();
    h.run_steps(3);
    assert!(!h.state().app.spiral_shown);
    assert_eq!(h.state().app.spiral.as_ref().unwrap().tapped, None);
    h.state_mut().app.close_spiral();
    h.run_steps(2);
    assert!(h.state().app.spiral.is_none());
    assert!(h.query_by_label("Espiral").is_none());
}

/// The spiral with a C major chord, as a PNG (needs a GPU adapter; run by hand:
/// `NOMAD_SHOTS=/some/dir cargo test -p nomad-app --features shots --test spiral shots -- --ignored`).
#[cfg(feature = "shots")]
#[test]
#[ignore = "renders with wgpu; run by hand for screenshots"]
fn shots() {
    let dir =
        std::env::var("NOMAD_SHOTS").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
    let mut h = harness();
    open(&mut h);
    for note in [48, 52, 55] {
        h.state_mut().app.model.send(Command::NoteOn {
            note,
            velocity: 0.8,
        });
    }
    for _ in 0..10 {
        step(&mut h);
    }
    let path = format!("{dir}/nomad-spiral.png");
    h.render().expect("render").save(&path).unwrap();
    println!("saved {path}");
}
