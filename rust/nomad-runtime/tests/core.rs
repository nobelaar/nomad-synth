use nomad_core::engine::{Engine, InstrumentId, Kind, MAX_INSTANCES, build_instance};
use nomad_core::params::pid;
use nomad_core::synth::Control;
use nomad_runtime::{Command, ControlHandle, create};

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

fn scene() -> Vec<Command> {
    vec![
        Command::SetBpm {
            id: DRUMS,
            bpm: 120.0,
        },
        Command::Start(DRUMS),
        Command::NoteOn {
            note: 60,
            velocity: 0.8,
        },
        Command::NoteOn {
            note: 67,
            velocity: 0.6,
        },
        Command::SetLevel {
            id: DRUMS,
            level: 0.7,
        },
    ]
}

/// Render `total` frames through the core in callbacks of `size`, as f32 mono.
fn render_core(cmds: Vec<Command>, size: usize, total: usize) -> Vec<f32> {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    for c in cmds {
        assert!(ctl.send(c));
    }
    let mut out = Vec::new();
    while out.len() < total {
        let mut buf = vec![0.0f32; size.min(total - out.len())];
        core.render_stereo(&mut buf, 1);
        out.extend(buf);
    }
    out
}

#[test]
fn matches_direct_engine_bit_exact() {
    let total = 6000;
    for size in [1, 37, 512, 1000] {
        // Direct engine with the same callback sizes and the same commands.
        let mut e = Engine::new(SR, 8);
        e.set_bpm(DRUMS, 120.0);
        e.start(DRUMS);
        // Command velocities are f32: feed the engine the same values.
        e.note_on(60, f64::from(0.8f32));
        e.note_on(67, f64::from(0.6f32));
        e.set_level(DRUMS, 0.7);
        let mut want = Vec::new();
        while want.len() < total {
            let mut b = vec![0.0; size.min(total - want.len())];
            e.render(&mut b);
            want.extend(b.iter().map(|&x| x as f32));
        }
        assert_eq!(render_core(scene(), size, total), want, "callback {size}");
    }
}

#[test]
fn command_order_is_preserved() {
    // Level 0 then 1 must end at 1; the reverse ends silent.
    let a = render_core(
        vec![
            Command::NoteOn {
                note: 60,
                velocity: 1.0,
            },
            Command::SetLevel {
                id: SYNTH,
                level: 0.0,
            },
            Command::SetLevel {
                id: SYNTH,
                level: 1.0,
            },
        ],
        512,
        4096,
    );
    let b = render_core(
        vec![
            Command::NoteOn {
                note: 60,
                velocity: 1.0,
            },
            Command::SetLevel {
                id: SYNTH,
                level: 1.0,
            },
            Command::SetLevel {
                id: SYNTH,
                level: 0.0,
            },
        ],
        512,
        4096,
    );
    assert!(a[2048..].iter().any(|&x| x != 0.0));
    assert!(b[2048..].iter().all(|&x| x == 0.0));
}

#[test]
fn full_queue_drops_and_counts() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let extra = 10;
    let mut refused = 0;
    for _ in 0..4096 + extra {
        if !ctl.send(Command::NoteOn {
            note: 60,
            velocity: 0.5,
        }) {
            refused += 1;
        }
    }
    assert_eq!(refused, extra);
    let t = ctl.telemetry();
    assert_eq!(
        t.dropped_commands
            .load(std::sync::atomic::Ordering::Relaxed),
        extra as u64
    );
    let mut buf = vec![0.0f32; 512];
    for _ in 0..20 {
        core.render_stereo(&mut buf, 1);
        assert!(buf.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }
    assert!(ctl.send(Command::Panic));
}

#[test]
fn invalid_values_do_not_panic() {
    let cmds = vec![
        Command::NoteOn {
            note: 200,
            velocity: f32::NAN,
        },
        Command::NoteOff { note: 255 },
        Command::SetParam {
            id: SYNTH,
            param: 999,
            value: f64::NAN,
        },
        Command::SetParam {
            id: SYNTH,
            param: pid::FILT_CUTOFF as u16,
            value: f64::INFINITY,
        },
        Command::SetParam {
            id: 77,
            param: 0,
            value: 1.0,
        },
        Command::SetControl {
            ctl: Control::Bend,
            value: f64::NEG_INFINITY,
        },
        Command::SetMaster(f64::NAN),
        Command::SetTargetSynth(DRUMS),
        Command::Start(99),
        Command::SetBpm {
            id: DRUMS,
            bpm: f64::NAN,
        },
        Command::SetLevel {
            id: SYNTH,
            level: f64::INFINITY,
        },
        Command::SetSource {
            id: SYNTH,
            src: Some(SYNTH),
        },
        Command::SeqSetStep {
            id: SYNTH,
            step: 200,
            on: true,
            note: Some(255),
            vel: Some(0),
        },
        Command::SeqAddNote {
            id: SYNTH,
            step: 99,
            note: 60,
        },
        Command::SeqTranspose {
            id: SYNTH,
            step: 0,
            delta: i16::MIN,
        },
        Command::SeqGate {
            id: SYNTH,
            gate: f64::NAN,
        },
        Command::SeqDivision {
            id: SYNTH,
            division: -1.0,
        },
        Command::SeqLength {
            id: SYNTH,
            length: 0,
        },
        Command::SeqSwing {
            id: SYNTH,
            swing: f64::INFINITY,
        },
        Command::SeqEnabled {
            id: DRUMS,
            enabled: true,
        },
        Command::SeqArm { id: 50, on: true },
        Command::SeqRest(SYNTH),
        Command::DrumSetStep {
            id: DRUMS,
            track: 9,
            step: 99,
            level: 9,
        },
        Command::DrumTrigger {
            id: DRUMS,
            track: 7,
            velocity: f64::NAN,
        },
        Command::DrumValue {
            id: DRUMS,
            param: 500,
            value: 1.0,
        },
        Command::DrumValue {
            id: DRUMS,
            param: 0,
            value: f64::NAN,
        },
        Command::DrumClear(SYNTH),
        Command::RemoveInstrument(99),
        Command::DropClosed,
        Command::SeqEnabled {
            id: SYNTH,
            enabled: true,
        },
        Command::Start(SYNTH),
    ];
    let out = render_core(cmds, 256, 8192);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
}

#[test]
fn stereo_and_channel_layouts() {
    let mono = render_core(scene(), 512, 2048);
    for ch in [1usize, 2, 6] {
        let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
        for c in scene() {
            ctl.send(c);
        }
        let mut out = Vec::new();
        for _ in 0..4 {
            let mut buf = vec![9.0f32; 512 * ch];
            core.render_stereo(&mut buf, ch);
            out.extend(buf);
        }
        for (f, frame) in out.chunks(ch).enumerate() {
            assert_eq!(frame[0], mono[f]);
            if ch > 1 {
                assert_eq!(frame[1], mono[f]);
            }
            assert!(frame[2.min(ch)..].iter().all(|&x| x == 0.0), "ch {ch}");
        }
    }
}

#[test]
fn garbage_returns_removed_instances_and_old_shots() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut buf = vec![0.0f32; 256];
    core.render_stereo(&mut buf, 1);
    // Tom (3) is not playing: the old shot comes back whole.
    ctl.send(Command::DrumSetBank {
        id: DRUMS,
        track: 3,
        shot: Box::new(vec![0.5; 100]),
    });
    ctl.send(Command::RemoveInstrument(SYNTH));
    core.render_stereo(&mut buf, 1);
    ctl.send(Command::DropClosed);
    core.render_stereo(&mut buf, 1);
    let mut shot_len = 0;
    let mut instances = 0;
    let n = ctl.reap_with(|g| match g {
        nomad_runtime::Garbage::Shot(s) => shot_len = s.len(),
        nomad_runtime::Garbage::Instance(_) => instances += 1,
    });
    assert_eq!((n, instances), (2, 1));
    assert!(shot_len > 1000, "the old tom shot, intact");
}

#[test]
fn add_when_full_returns_box_via_garbage() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    let mut e_ids = Engine::new(SR, 8); // only to mint ids and names
    for _ in 0..MAX_INSTANCES - 1 {
        let (id, name) = e_ids.next_id(Kind::Synth);
        ctl.send(Command::AddInstrument(build_instance(
            SR,
            1,
            Kind::Synth,
            id + 100,
            name,
        )));
    }
    let mut buf = vec![0.0f32; 64];
    core.render_stereo(&mut buf, 1);
    assert_eq!(ctl.reap(), MAX_INSTANCES - 1 - (MAX_INSTANCES - 2));
}

#[test]
fn panic_in_render_is_contained() {
    let (mut core, mut ctl) = create(Engine::new(SR, 8), SR);
    ctl.send(Command::NoteOn {
        note: 60,
        velocity: 1.0,
    });
    let mut buf = vec![0.0f32; 512];
    core.render_stereo(&mut buf, 1);
    ctl.send(Command::TestPanic);
    let mut buf = vec![7.0f32; 512];
    core.render_stereo(&mut buf, 2);
    assert!(buf.iter().all(|&x| x == 0.0));
    assert_eq!(
        ctl.telemetry()
            .errors
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    // The core keeps running afterwards.
    let mut buf = vec![0.0f32; 512];
    core.render_stereo(&mut buf, 1);
    assert!(buf.iter().any(|&x| x != 0.0));
}

#[test]
fn handles_are_send() {
    fn is_send<T: Send>() {}
    is_send::<ControlHandle>();
    is_send::<nomad_runtime::AudioCore>();
    is_send::<Command>();
}

/// Callback size is invisible (to float rounding) while no ramp is in flight. The
/// oscillator phase is recomputed per block, so other partitions round differently
/// (~1e-10). Level, mute, master and smoothed parameters ramp over one control
/// block (as in Python), so a callback shorter than 256 frames ramps faster: that
/// part is by design and not covered here.
#[test]
fn callback_size_does_not_change_the_audio() {
    let total = 6000;
    let sc = || {
        vec![
            Command::SetBpm {
                id: DRUMS,
                bpm: 120.0,
            },
            Command::Start(DRUMS),
            Command::NoteOn {
                note: 60,
                velocity: 0.8,
            },
            Command::NoteOn {
                note: 67,
                velocity: 0.6,
            },
        ]
    };
    let big = render_core(sc(), total, total);
    for size in [1, 37, 512, 1000] {
        let got = render_core(sc(), size, total);
        let max = got
            .iter()
            .zip(&big)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max <= 1e-6, "callback {size}: {max:e}");
    }
}
