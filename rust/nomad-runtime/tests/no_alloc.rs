//! `AudioCore::render_stereo` must not allocate, whatever the commands: a counting global allocator that only
//! counts while a thread-local flag is on, plus a backtrace of the first hit.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;

use nomad_core::engine::{Engine, InstrumentId, Kind, build_instance};
use nomad_core::params::{DRUM_PARAMS, pid};
use nomad_core::synth::Control;
use nomad_runtime::{Command, create};

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
static FIRST: Mutex<Option<String>> = Mutex::new(None);

fn hit() {
    if ON.with(|f| f.get()) {
        ON.with(|f| f.set(false)); // nested allocations below are not counted
        COUNT.with(|c| c.set(c.get() + 1));
        let mut first = FIRST.lock().unwrap();
        if first.is_none() {
            *first = Some(std::backtrace::Backtrace::force_capture().to_string());
        }
        drop(first);
        ON.with(|f| f.set(true));
    }
}

// SAFETY: forwards every call to the system allocator unchanged; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        hit();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        hit();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        hit();
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Run `f` and fail with the first allocation's backtrace if it allocated or freed.
fn assert_no_alloc<R>(what: &str, f: impl FnOnce() -> R) -> R {
    *FIRST.lock().unwrap() = None;
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    let r = f();
    ON.with(|o| o.set(false));
    let n = COUNT.with(|c| c.get());
    if n > 0 {
        let bt = FIRST.lock().unwrap().take().unwrap_or_default();
        panic!("{what}: {n} allocator calls; first at:\n{bt}");
    }
    r
}

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

#[test]
fn audio_core_render_is_allocation_free() {
    let mut e = Engine::new(SR, 8);
    let (new_id, name) = e.next_id(Kind::Drums);
    let new_inst = build_instance(SR, 8, Kind::Drums, new_id, name);
    let (mut core, mut ctl) = create(e, SR);
    let swing = DRUM_PARAMS.iter().position(|s| s.name == "swing").unwrap() as u16;
    let kick_tune = 0u16;
    let bpm = DRUM_PARAMS.iter().position(|s| s.name == "bpm").unwrap() as u16;
    // Every variant (except the test-only panic), some with bad values.
    let cmds = vec![
        Command::Start(DRUMS),
        Command::Start(SYNTH),
        Command::NoteOn {
            note: 60,
            velocity: 0.8,
        },
        Command::NoteOn {
            note: 64,
            velocity: 0.8,
        },
        Command::NoteOff { note: 64 },
        Command::SetParam {
            id: SYNTH,
            param: pid::FILT_CUTOFF as u16,
            value: 900.0,
        },
        Command::SetParam {
            id: SYNTH,
            param: pid::POLYPHONY as u16,
            value: 4.0,
        },
        Command::SetControl {
            ctl: Control::Bend,
            value: 0.3,
        },
        Command::SetMaster(0.5),
        Command::SetTargetSynth(SYNTH),
        Command::SetBpm {
            id: DRUMS,
            bpm: 128.0,
        },
        Command::SetMute {
            id: DRUMS,
            muted: true,
        },
        Command::SetMute {
            id: DRUMS,
            muted: false,
        },
        Command::SetLevel {
            id: SYNTH,
            level: 0.8,
        },
        Command::SetSource {
            id: SYNTH,
            src: Some(DRUMS),
        },
        Command::SeqSetStep {
            id: SYNTH,
            step: 0,
            on: true,
            note: Some(60),
            vel: Some(100),
        },
        Command::SeqAddNote {
            id: SYNTH,
            step: 0,
            note: 64,
        },
        Command::SeqTranspose {
            id: SYNTH,
            step: 0,
            delta: 2,
        },
        Command::SeqGate {
            id: SYNTH,
            gate: 0.6,
        },
        Command::SeqDivision {
            id: SYNTH,
            division: 0.5,
        },
        Command::SeqLength {
            id: SYNTH,
            length: 8,
        },
        Command::SeqSwing {
            id: SYNTH,
            swing: 0.2,
        },
        Command::SeqEnabled {
            id: SYNTH,
            enabled: true,
        },
        Command::SeqArm {
            id: SYNTH,
            on: true,
        },
        Command::SeqRest(SYNTH),
        Command::SeqArm {
            id: SYNTH,
            on: false,
        },
        Command::DrumSetStep {
            id: DRUMS,
            track: 2,
            step: 3,
            level: 2,
        },
        Command::DrumTrigger {
            id: DRUMS,
            track: 5,
            velocity: 0.9,
        },
        Command::DrumValue {
            id: DRUMS,
            param: swing,
            value: 0.3,
        },
        Command::DrumValue {
            id: DRUMS,
            param: kick_tune,
            value: 0.4,
        },
        Command::DrumValue {
            id: DRUMS,
            param: bpm,
            value: 140.0,
        },
        Command::DrumSetBank {
            id: DRUMS,
            track: 0,
            shot: Box::new(vec![0.1; 2000]),
        },
        Command::DrumSetBank {
            id: 99,
            track: 0,
            shot: Box::new(vec![0.1; 20]),
        },
        Command::AddInstrument(new_inst),
        Command::Start(new_id),
        Command::SetSource {
            id: SYNTH,
            src: None,
        },
        Command::RemoveInstrument(new_id),
        Command::DrumClear(DRUMS),
        Command::SeqClear(SYNTH),
        Command::Panic,
        Command::DropClosed,
    ];
    let mut out = vec![0.0f32; 512 * 2];
    let mut cmds = cmds.into_iter();
    // Prime one callback, then send a few commands per callback while rendering.
    core.render_stereo(&mut out, 2);
    loop {
        let batch: Vec<Command> = cmds.by_ref().take(3).collect();
        if batch.is_empty() {
            break;
        }
        for c in batch {
            assert!(ctl.send(c));
        }
        assert_no_alloc("render with commands", || core.render_stereo(&mut out, 2));
    }
    assert_no_alloc("idle renders", || {
        for _ in 0..100 {
            core.render_stereo(&mut out, 2);
        }
    });
    assert!(
        ctl.reap() >= 3,
        "old shots and the closed instance come back"
    );
}

/// The counter itself works: an allocation inside the guard is caught.
#[test]
#[should_panic(expected = "allocator calls")]
fn counter_detects_allocation() {
    assert_no_alloc("self-test", || drop(std::hint::black_box(vec![1u8; 10])));
}

/// Publishing the UI mirrors at the end of each callback is stores only.
#[test]
fn mirror_updates_do_not_allocate() {
    let e = Engine::new(SR, 8);
    let shared = e.shared(SYNTH).unwrap();
    let (mut core, mut ctl) = create(e, SR);
    for c in [
        Command::Start(SYNTH),
        Command::Start(DRUMS),
        Command::SeqEnabled {
            id: SYNTH,
            enabled: true,
        },
        Command::SeqSetStep {
            id: SYNTH,
            step: 0,
            on: true,
            note: Some(60),
            vel: None,
        },
        Command::NoteOn {
            note: 64,
            velocity: 0.8,
        },
    ] {
        ctl.send(c);
    }
    let mut out = vec![0.0f32; 512 * 2];
    core.render_stereo(&mut out, 2);
    assert_no_alloc("render with mirrors", || {
        for _ in 0..200 {
            core.render_stereo(&mut out, 2);
        }
    });
    assert!(shared.active_note(64));
}
