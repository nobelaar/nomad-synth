use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nomad_core::engine::{Engine, Kind};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::midi::input::{MidiHandler, MidiInput, contain, list_input_ports};
use nomad_runtime::midi::map::MidiMap;
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;
const PORT: &str = "TEST:TEST MIDI 1 24:0";

fn render(core: &mut AudioCore, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames];
    for chunk in out.chunks_mut(512) {
        core.render_stereo(chunk, 1);
    }
    out
}

fn peak(v: &[f32]) -> f32 {
    v.iter().fold(0.0, |m, x| m.max(x.abs()))
}

#[test]
fn list_ports_does_not_panic() {
    let _ = list_input_ports();
}

#[test]
fn sender_drops_and_counts_when_full() {
    let (_core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut tx = ctl.midi_sender().unwrap();
    assert!(ctl.midi_sender().is_none(), "handed out once");
    let n = 7;
    let refused = (0..1024 + n)
        .filter(|_| {
            !tx.send(Command::NoteOn {
                note: 60,
                velocity: 0.5,
            })
        })
        .count();
    assert_eq!(refused, n);
    assert_eq!(ctl.telemetry().midi_dropped.load(Relaxed), n as u64);
}

#[test]
fn router_sink_maps_events_to_commands() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    let mut h = MidiHandler::new(ctl.midi_sender().unwrap(), map, PORT.into());
    assert_eq!(peak(&render(&mut core, 2048)), 0.0);
    h.feed(&[0x90, 60, 100]);
    let on = render(&mut core, 4800);
    assert!(peak(&on) > 0.05);
    h.feed(&[0x80, 60, 0]);
    render(&mut core, 48000); // release (400 ms) finishes
    assert_eq!(peak(&render(&mut core, 2048)), 0.0);
    assert_eq!(ctl.telemetry().midi_events.load(Relaxed), 2);
    h.feed(&[0xF8]); // clock: ignored, not counted
    assert_eq!(ctl.telemetry().midi_events.load(Relaxed), 2);
}

#[test]
fn cc_map_sets_param_on_target_synth() {
    let mut engine = Engine::new(SR, 8);
    let second = engine.add_instrument(Kind::Synth);
    let (mut core, mut ctl) = create(engine, SR);
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    map.lock().unwrap().arm(Some("filt_cutoff")).unwrap();
    let mut h = MidiHandler::new(ctl.midi_sender().unwrap(), Arc::clone(&map), PORT.into());
    h.feed(&[0xB0, 21, 127]); // learns CC 21 and applies it
    render(&mut core, 512);
    let hi = SYNTH_PARAMS[pid::FILT_CUTOFF].hi;
    let get = |core: &mut AudioCore, id| {
        core.engine_mut().synth_mut(id).unwrap().values()[pid::FILT_CUTOFF]
    };
    assert!((get(&mut core, 1) - hi).abs() < 1e-9);
    // Retarget: the same knob now moves the second synth only.
    assert!(ctl.send(Command::SetTargetSynth(second)));
    render(&mut core, 512);
    assert_eq!(ctl.telemetry().target_synth.load(Relaxed), second);
    h.feed(&[0xB0, 21, 0]);
    render(&mut core, 512);
    assert!((get(&mut core, second) - SYNTH_PARAMS[pid::FILT_CUTOFF].lo).abs() < 1e-9);
    assert!((get(&mut core, 1) - hi).abs() < 1e-9);
}

#[test]
fn cc_mapped_to_master_volume_goes_to_the_mixer() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    map.lock().unwrap().arm(Some("master_volume")).unwrap();
    let mut h = MidiHandler::new(ctl.midi_sender().unwrap(), map, PORT.into());
    h.feed(&[0x90, 60, 127]);
    render(&mut core, 4800);
    h.feed(&[0xB0, 7, 0]);
    // The master is smoothed (15 ms time constant): after 200 ms it is ~1e-6 of before.
    let out = render(&mut core, 9600);
    assert!(
        peak(&out[9088..]) < 1e-4,
        "master at 0 silences the mix: {}",
        peak(&out[9088..])
    );
}

#[test]
fn callback_panic_is_contained() {
    let (_core, ctl) = create(Engine::new(SR, 8), SR);
    let tel = ctl.telemetry();
    contain(&tel, || panic!("boom in the MIDI callback"));
    contain(&tel, || {});
    assert_eq!(tel.midi_errors.load(Relaxed), 1);
}

#[test]
#[ignore = "requires ALSA sequencer"]
fn virtual_port_roundtrip() {
    use midir::os::unix::VirtualOutput;
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut out = midir::MidiOutput::new("nomad-test")
        .unwrap()
        .create_virtual("nomad-virtual-out")
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    let input =
        MidiInput::open(Some("nomad-virtual-out"), ctl.midi_sender().unwrap(), map).unwrap();
    assert!(
        input.port_name().contains("nomad-virtual-out"),
        "{}",
        input.port_name()
    );
    out.send(&[0x90, 60, 100]).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(ctl.telemetry().midi_events.load(Relaxed), 1);
    assert!(peak(&render(&mut core, 4800)) > 0.05);
    assert!(input.is_alive());
}

#[test]
#[ignore = "requires ALSA sequencer"]
fn reset_releases_held_keys() {
    use midir::os::unix::VirtualOutput;
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut out = midir::MidiOutput::new("nomad-test2")
        .unwrap()
        .create_virtual("nomad-virtual-reset")
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    let input =
        MidiInput::open(Some("nomad-virtual-reset"), ctl.midi_sender().unwrap(), map).unwrap();
    out.send(&[0x90, 60, 100]).unwrap();
    out.send(&[0x90, 64, 100]).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    render(&mut core, 4800);
    input.reset();
    render(&mut core, 48000);
    assert_eq!(
        peak(&render(&mut core, 2048)),
        0.0,
        "held keys were released"
    );
}

#[test]
#[ignore = "requires ALSA sequencer"]
fn close_returns_sender_and_releases_keys() {
    use midir::os::unix::VirtualOutput;
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut out = midir::MidiOutput::new("nomad-test3")
        .unwrap()
        .create_virtual("nomad-virtual-close")
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let map = Arc::new(Mutex::new(MidiMap::new(None)));
    let input = MidiInput::open(
        Some("nomad-virtual-close"),
        ctl.midi_sender().unwrap(),
        Arc::clone(&map),
    )
    .unwrap();
    out.send(&[0x90, 60, 100]).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    render(&mut core, 4800);
    let sender = input.close().expect("the sender comes back");
    render(&mut core, 48000);
    assert_eq!(peak(&render(&mut core, 2048)), 0.0, "held key released");
    // The same sender opens the port again.
    let again = MidiInput::open(Some("nomad-virtual-close"), sender, map).unwrap();
    assert!(again.is_alive());
}
