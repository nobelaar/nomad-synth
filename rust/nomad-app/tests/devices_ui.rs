//! Hot audio-device switching with a fake device: the engine survives, a device
//! that does not open falls back to the previous one.
// The opener hands the core back by value, like the real one.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use nomad_app::devices::OutputDevice;
use nomad_app::model::{AppModel, initial_views};
use nomad_app::ui::NomadApp;
use nomad_core::engine::{Engine, Kind};
use nomad_core::params::pid;
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;
const BAD: usize = 9;
const LONG: usize = 10;

type Slot = Rc<RefCell<Option<AudioCore>>>;

/// A "device" that just holds the core where the test can render it.
struct Fake(Slot);

impl OutputDevice for Fake {
    fn close(self: Box<Self>) -> AudioCore {
        self.0.borrow_mut().take().expect("core in the fake device")
    }
    fn stop(&mut self) {}
}

fn render(slot: &Slot) {
    let mut out = vec![0.0f32; 512];
    slot.borrow_mut()
        .as_mut()
        .expect("a device holds the core")
        .render_stereo(&mut out, 1);
}

fn app() -> (NomadApp, Slot, Rc<RefCell<Vec<Option<usize>>>>) {
    let e = Engine::new(SR, 8);
    let views = initial_views(&e);
    let (core, ctl) = create(e, SR);
    let tel = ctl.telemetry();
    let model = AppModel::new(ctl, SR, views);
    let mut app = NomadApp::new(
        model,
        None,
        None,
        Arc::new(Mutex::new(MidiMap::new(None))),
        tel,
    );
    let slot: Slot = Rc::new(RefCell::new(Some(core)));
    let opened = Rc::new(RefCell::new(Vec::new()));
    app.output = Some(Box::new(Fake(Rc::clone(&slot))));
    app.audio_device = Some(0);
    let (s, log) = (Rc::clone(&slot), Rc::clone(&opened));
    app.opener = Box::new(move |core, index| {
        log.borrow_mut().push(index);
        if index == Some(BAD) {
            return Err(("Could not open the audio device: busy".into(), core));
        }
        if index == Some(LONG) {
            let e =
                "Could not open the audio device: Buffer size 512 is not in the supported range";
            return Err((e.into(), core));
        }
        *s.borrow_mut() = Some(core);
        Ok(Box::new(Fake(Rc::clone(&s))) as Box<dyn OutputDevice>)
    });
    (app, slot, opened)
}

#[test]
fn device_menu_switch_keeps_patterns() {
    let (mut app, slot, opened) = app();
    let drums = app.model.add_instrument(Kind::Drums).unwrap();
    app.model.send(Command::DrumSetStep {
        id: drums,
        track: 2,
        step: 7,
        level: 2,
    });
    app.model.send(Command::SetParam {
        id: 1,
        param: pid::FILT_CUTOFF as u16,
        value: 1234.0,
    });
    render(&slot);
    app.switch_audio(1);
    assert_eq!(*opened.borrow(), [Some(1)]);
    assert_eq!(app.audio_device, Some(1));
    assert_eq!(app.status, "");
    render(&slot);
    let mut core = slot.borrow_mut().take().unwrap();
    let e = core.engine_mut();
    assert_eq!(e.instruments().len(), 3, "the added machine survived");
    assert_eq!(e.drums_mut(drums).unwrap().steps[2][7], 2);
    assert_eq!(e.synth_mut(1).unwrap().values()[pid::FILT_CUTOFF], 1234.0);
}

#[test]
fn failed_switch_restores_previous_and_reports() {
    let (mut app, slot, opened) = app();
    app.model.send(Command::DrumSetStep {
        id: 2,
        track: 0,
        step: 3,
        level: 1,
    });
    render(&slot);
    app.switch_audio(BAD);
    assert_eq!(*opened.borrow(), [Some(BAD), Some(0)], "tried, then back");
    assert_eq!(app.audio_device, Some(0));
    assert!(
        app.status.starts_with("AUDIO: Could not open"),
        "{}",
        app.status
    );
    assert!(app.output.is_some());
    render(&slot);
    let mut core = slot.borrow_mut().take().unwrap();
    assert_eq!(core.engine_mut().drums_mut(2).unwrap().steps[0][3], 1);
}

#[test]
fn midi_switch_without_sender_reports() {
    let (mut app, _slot, _) = app();
    app.switch_midi("nothing");
    assert_eq!(app.status, "MIDI: no se puede abrir otro puerto");
}

#[test]
fn long_error_keeps_the_fallback_suffix() {
    let (mut app, _slot, _) = app();
    app.switch_audio(LONG);
    assert!(
        app.status.ends_with(" (sigue el anterior)"),
        "{}",
        app.status
    );
    assert!(app.status.starts_with("AUDIO: Could not open"));
    assert!(app.status.chars().count() <= 80, "still one short line");
}
