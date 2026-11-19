//! Audio-to-UI state mirrors: the audio thread publishes what the interface
//! reads with relaxed atomic stores only (no locks, no allocation). Words are
//! not coherent with each other (a torn read is acceptable, like the scope).
//!
//! Lives in `nomad-core` because every `Instance` carries its mirror; the
//! runtime re-exports it as `nomad_runtime::mirror`.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64};

use crate::clock::STEPS;
use crate::engine::{InstrumentId, Kind};
use crate::params::pid;
use crate::sequencer::EXTRA;

const TRACKS: usize = 6;

/// `f64` stored as its bits.
#[derive(Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub fn new(v: f64) -> AtomicF64 {
        AtomicF64(AtomicU64::new(v.to_bits()))
    }
    pub fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Relaxed))
    }
    pub fn set(&self, v: f64) {
        self.0.store(v.to_bits(), Relaxed);
    }
}

fn u8s<const N: usize>() -> [AtomicU8; N] {
    std::array::from_fn(|_| AtomicU8::new(0))
}

fn f64s<const N: usize>() -> [AtomicF64; N] {
    std::array::from_fn(|_| AtomicF64::new(0.0))
}

/// Everything the UI reads about one instance (always inside an `Arc`).
pub struct InstrumentShared {
    pub id: InstrumentId,
    pub kind: Kind,
    /// Source instance id, 0 = Internal.
    pub source: AtomicU32,
    pub clock_running: AtomicBool,
    pub clock_bpm: AtomicF64,
    pub clock_pos: AtomicF64,
    pub eff_running: AtomicBool,
    pub eff_bpm: AtomicF64,
    pub eff_pos: AtomicF64,
    pub level: AtomicF64,
    pub muted: AtomicBool,
    pub closing: AtomicBool,
    /// Pattern step under the effective clock's playhead, -1 when stopped.
    pub step: AtomicI32,
    // Synth
    pub params: [AtomicF64; pid::COUNT],
    pub seq_on: [AtomicU8; STEPS],
    pub seq_note: [AtomicU8; STEPS],
    pub seq_vel: [AtomicU8; STEPS],
    pub seq_extra: [[AtomicU8; EXTRA]; STEPS],
    pub seq_gate: AtomicF64,
    pub seq_division: AtomicF64,
    pub seq_swing: AtomicF64,
    pub seq_length: AtomicU8,
    pub seq_enabled: AtomicBool,
    pub seq_rec: AtomicBool,
    pub seq_rec_step: AtomicU8,
    /// Held keys, a 128-bit mask in two words.
    pub active_notes: [AtomicU64; 2],
    pub voices: AtomicU8,
    // Drums
    pub values: [AtomicF64; 20],
    pub steps: [[AtomicU8; STEPS]; TRACKS],
    pub playing: AtomicBool,
}

/// A copy of a synth's sequencer as the mirror holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqSnapshot {
    pub on: [u8; STEPS],
    pub note: [u8; STEPS],
    pub vel: [u8; STEPS],
    pub extra: [[u8; EXTRA]; STEPS],
    pub gate: f64,
    pub division: f64,
    pub swing: f64,
    pub length: u8,
    pub enabled: bool,
    pub rec: bool,
    pub rec_step: u8,
}

/// A copy of a drum machine as the mirror holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrumSnapshot {
    pub values: [f64; 20],
    pub steps: [[u8; STEPS]; TRACKS],
    pub playing: bool,
}

impl InstrumentShared {
    pub fn new(id: InstrumentId, kind: Kind) -> InstrumentShared {
        InstrumentShared {
            id,
            kind,
            source: AtomicU32::new(0),
            clock_running: AtomicBool::new(false),
            clock_bpm: AtomicF64::new(110.0),
            clock_pos: AtomicF64::new(0.0),
            eff_running: AtomicBool::new(false),
            eff_bpm: AtomicF64::new(110.0),
            eff_pos: AtomicF64::new(0.0),
            level: AtomicF64::new(1.0),
            muted: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            step: AtomicI32::new(-1),
            params: f64s(),
            seq_on: u8s(),
            seq_note: u8s(),
            seq_vel: u8s(),
            seq_extra: std::array::from_fn(|_| u8s()),
            seq_gate: AtomicF64::new(0.5),
            seq_division: AtomicF64::new(0.25),
            seq_swing: AtomicF64::new(0.0),
            seq_length: AtomicU8::new(STEPS as u8),
            seq_enabled: AtomicBool::new(false),
            seq_rec: AtomicBool::new(false),
            seq_rec_step: AtomicU8::new(0),
            active_notes: [AtomicU64::new(0), AtomicU64::new(0)],
            voices: AtomicU8::new(0),
            values: f64s(),
            steps: std::array::from_fn(|_| u8s()),
            playing: AtomicBool::new(false),
        }
    }

    pub fn param(&self, id: usize) -> f64 {
        self.params.get(id).map_or(f64::NAN, AtomicF64::get)
    }

    pub fn active_note(&self, n: u8) -> bool {
        n < 128 && (self.active_notes[usize::from(n / 64)].load(Relaxed) >> (n % 64)) & 1 == 1
    }

    pub fn seq_snapshot(&self, out: &mut SeqSnapshot) {
        for k in 0..STEPS {
            out.on[k] = self.seq_on[k].load(Relaxed);
            out.note[k] = self.seq_note[k].load(Relaxed);
            out.vel[k] = self.seq_vel[k].load(Relaxed);
            for j in 0..EXTRA {
                out.extra[k][j] = self.seq_extra[k][j].load(Relaxed);
            }
        }
        out.gate = self.seq_gate.get();
        out.division = self.seq_division.get();
        out.swing = self.seq_swing.get();
        out.length = self.seq_length.load(Relaxed);
        out.enabled = self.seq_enabled.load(Relaxed);
        out.rec = self.seq_rec.load(Relaxed);
        out.rec_step = self.seq_rec_step.load(Relaxed);
    }

    pub fn drum_snapshot(&self, out: &mut DrumSnapshot) {
        for (o, v) in out.values.iter_mut().zip(&self.values) {
            *o = v.get();
        }
        for (o, s) in out.steps.iter_mut().zip(&self.steps) {
            for (x, a) in o.iter_mut().zip(s) {
                *x = a.load(Relaxed);
            }
        }
        out.playing = self.playing.load(Relaxed);
    }
}
