//! Messages to the audio thread and the objects it hands back for freeing.

use nomad_core::engine::{Instance, InstrumentId};
use nomad_core::synth::Control;

/// One engine operation, applied by the audio thread at the start of a callback.
/// Values are sanitised like the direct `Engine` calls; unknown ids are ignored.
/// Heap payloads (`Box`) are built by the sender and come back as [`Garbage`].
// The hidden `TestPanic` is a test hook, not a non-exhaustive marker.
#[allow(clippy::manual_non_exhaustive)]
pub enum Command {
    NoteOn {
        note: u8,
        velocity: f32,
    },
    NoteOff {
        note: u8,
    },
    Panic,
    /// Synth parameter by `pid` index on synth `id`.
    SetParam {
        id: InstrumentId,
        param: u16,
        value: f64,
    },
    /// Bend or mod wheel on the target synth.
    SetControl {
        ctl: Control,
        value: f64,
    },
    SetMaster(f64),
    SetTargetSynth(InstrumentId),
    Start(InstrumentId),
    Stop(InstrumentId),
    SetBpm {
        id: InstrumentId,
        bpm: f64,
    },
    SetMute {
        id: InstrumentId,
        muted: bool,
    },
    SetLevel {
        id: InstrumentId,
        level: f64,
    },
    SetSource {
        id: InstrumentId,
        src: Option<InstrumentId>,
    },
    SeqSetStep {
        id: InstrumentId,
        step: u8,
        on: bool,
        note: Option<u8>,
        vel: Option<u8>,
    },
    SeqAddNote {
        id: InstrumentId,
        step: u8,
        note: u8,
    },
    SeqTranspose {
        id: InstrumentId,
        step: u8,
        delta: i16,
    },
    SeqClear(InstrumentId),
    SeqGate {
        id: InstrumentId,
        gate: f64,
    },
    SeqDivision {
        id: InstrumentId,
        division: f64,
    },
    SeqLength {
        id: InstrumentId,
        length: u8,
    },
    SeqSwing {
        id: InstrumentId,
        swing: f64,
    },
    SeqEnabled {
        id: InstrumentId,
        enabled: bool,
    },
    SeqArm {
        id: InstrumentId,
        on: bool,
    },
    SeqRest(InstrumentId),
    DrumSetStep {
        id: InstrumentId,
        track: u8,
        step: u8,
        level: u8,
    },
    DrumClear(InstrumentId),
    DrumTrigger {
        id: InstrumentId,
        track: u8,
        velocity: f64,
    },
    /// Drum parameter by `DRUM_PARAMS` index. Tempo reaches the clock; a tune or
    /// decay change only stores the value: the sender renders the new one-shot
    /// off the audio thread and sends `DrumSetBank`.
    DrumValue {
        id: InstrumentId,
        param: u16,
        value: f64,
    },
    DrumSetBank {
        id: InstrumentId,
        track: u8,
        shot: Box<Vec<f32>>,
    },
    AddInstrument(Box<nomad_core::engine::Instance>),
    RemoveInstrument(InstrumentId),
    DropClosed,
    /// Test hook: panics inside the callback to check containment.
    #[doc(hidden)]
    TestPanic,
}

/// Objects the audio thread no longer needs; freed by `ControlHandle::reap`.
pub enum Garbage {
    Instance(Box<Instance>),
    Shot(Box<Vec<f32>>),
}
