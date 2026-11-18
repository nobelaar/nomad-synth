use std::sync::atomic::Ordering::Relaxed;

use nomad_core::engine::{Engine, InstrumentId, Kind, build_instance};
use nomad_core::params::{SYNTH_PARAMS, pid};
use nomad_runtime::mirror::{DrumSnapshot, SeqSnapshot};
use nomad_runtime::{AudioCore, Command, create};

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

fn render(core: &mut AudioCore, frames: usize) {
    let mut out = vec![0.0f32; 512];
    for _ in 0..frames.div_ceil(512) {
        core.render_stereo(&mut out, 1);
    }
}

#[test]
fn params_mirror_follows_set_param() {
    let e = Engine::new(SR, 8);
    let m = e.shared(SYNTH).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    ctl.send(Command::SetParam {
        id: SYNTH,
        param: pid::FILT_CUTOFF as u16,
        value: 1e9,
    });
    render(&mut core, 512);
    assert_eq!(m.param(pid::FILT_CUTOFF), SYNTH_PARAMS[pid::FILT_CUTOFF].hi);
    assert!(m.param(999).is_nan());
}

#[test]
fn pattern_mirror_follows_edit_commands() {
    let e = Engine::new(SR, 8);
    let (ms, md) = (e.shared(SYNTH).unwrap(), e.shared(DRUMS).unwrap());
    let (mut core, mut ctl) = create(e, SR);
    ctl.send(Command::SeqSetStep {
        id: SYNTH,
        step: 3,
        on: true,
        note: Some(64),
        vel: Some(90),
    });
    ctl.send(Command::SeqAddNote {
        id: SYNTH,
        step: 3,
        note: 67,
    });
    ctl.send(Command::DrumSetStep {
        id: DRUMS,
        track: 2,
        step: 5,
        level: 2,
    });
    render(&mut core, 512);
    let mut s = SeqSnapshot::default();
    ms.seq_snapshot(&mut s);
    assert_eq!(
        (s.on[3], s.note[3], s.vel[3], s.extra[3][0]),
        (1, 64, 90, 67)
    );
    let mut d = DrumSnapshot::default();
    md.drum_snapshot(&mut d);
    assert_eq!(d.steps[2][5], 2);
    assert_eq!(d.values[18], 110.0, "bpm value");
}

#[test]
fn rec_written_pattern_reaches_mirror() {
    let e = Engine::new(SR, 8);
    let m = e.shared(SYNTH).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    let mut midi = ctl.midi_sender().unwrap();
    ctl.send(Command::SeqArm {
        id: SYNTH,
        on: true,
    });
    render(&mut core, 512);
    midi.send(Command::NoteOn {
        note: 62,
        velocity: 0.8,
    });
    midi.send(Command::NoteOff { note: 62 });
    render(&mut core, 512);
    let mut s = SeqSnapshot::default();
    m.seq_snapshot(&mut s);
    assert!(s.rec);
    assert_eq!((s.on[0], s.note[0], s.rec_step), (1, 62, 1));
}

#[test]
fn clock_and_follower_mirror() {
    let e = Engine::new(SR, 8);
    let m = e.shared(SYNTH).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    ctl.send(Command::SetBpm {
        id: DRUMS,
        bpm: 133.0,
    });
    ctl.send(Command::SetSource {
        id: SYNTH,
        src: Some(DRUMS),
    });
    ctl.send(Command::Start(DRUMS));
    render(&mut core, 2048);
    assert_eq!(m.source.load(Relaxed), DRUMS);
    assert_eq!(m.eff_bpm.get(), 133.0);
    assert!(m.eff_running.load(Relaxed) && !m.clock_running.load(Relaxed));
    assert!(m.eff_pos.get() > 0.0);
}

#[test]
fn playhead_step_advances_when_running() {
    let e = Engine::new(SR, 8);
    let m = e.shared(DRUMS).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    render(&mut core, 512);
    assert_eq!(m.step.load(Relaxed), -1);
    ctl.send(Command::SetBpm {
        id: DRUMS,
        bpm: 120.0,
    });
    ctl.send(Command::Start(DRUMS));
    render(&mut core, 512);
    assert_eq!(m.step.load(Relaxed), 0);
    render(&mut core, 6000 * 3); // a 16th is 6000 frames at 120 bpm
    assert!(m.step.load(Relaxed) >= 3, "{}", m.step.load(Relaxed));
}

#[test]
fn active_notes_mirror() {
    let e = Engine::new(SR, 8);
    let m = e.shared(SYNTH).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    ctl.send(Command::NoteOn {
        note: 60,
        velocity: 0.8,
    });
    ctl.send(Command::NoteOn {
        note: 64,
        velocity: 0.8,
    });
    render(&mut core, 512);
    assert!(m.active_note(60) && m.active_note(64) && !m.active_note(62) && !m.active_note(200));
    assert_eq!(m.voices.load(Relaxed), 2);
    assert_eq!(ctl.telemetry().voices_total.load(Relaxed), 2);
    assert!(f32::from_bits(ctl.telemetry().peak.load(Relaxed)) > 0.0);
    ctl.send(Command::NoteOff { note: 60 });
    ctl.send(Command::NoteOff { note: 64 });
    render(&mut core, 48000);
    assert!(!m.active_note(60) && !m.active_note(64));
    assert_eq!(m.voices.load(Relaxed), 0);
}

#[test]
fn add_and_remove_instance_lifecycle() {
    let mut e = Engine::new(SR, 8);
    let (id, name) = e.next_id(Kind::Drums);
    let inst = build_instance(SR, 8, Kind::Drums, id, name);
    let m = std::sync::Arc::clone(&inst.shared);
    let (mut core, mut ctl) = create(e, SR);
    ctl.send(Command::AddInstrument(inst));
    ctl.send(Command::DrumSetStep {
        id,
        track: 0,
        step: 7,
        level: 1,
    });
    render(&mut core, 512);
    let mut d = DrumSnapshot::default();
    m.drum_snapshot(&mut d);
    assert_eq!(d.steps[0][7], 1);
    ctl.send(Command::RemoveInstrument(id));
    render(&mut core, 512);
    assert!(m.closing.load(Relaxed));
    ctl.send(Command::DropClosed);
    render(&mut core, 512);
    ctl.reap();
    // The UI still holds its Arc: reading the dropped instance is safe.
    m.drum_snapshot(&mut d);
    assert_eq!(d.steps[0][7], 1);
}
