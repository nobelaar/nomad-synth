//! `MidiRouter` of `nomad/midi/midi_input.py`: events to engine calls, with the
//! sustain pedal and the all-notes-off / all-sound-off CCs. Keys are 128-bit
//! sets, so handling an event never allocates.
//!
//! shortcut: CC 1/64/120/123 are consumed here and not forwarded; every other
//! CC goes to `RouterSink::other_cc`.

use nomad_core::synth::Control;

use super::parse::{MidiEvent, MidiKind};

/// Where the router sends what it decides.
pub trait RouterSink {
    fn note_on(&mut self, note: u8, velocity: f64);
    fn note_off(&mut self, note: u8);
    fn set_control(&mut self, ctl: Control, value: f64);
    fn panic(&mut self);
    fn other_cc(&mut self, cc: u8, value: u8, channel: u8);
}

#[derive(Default)]
pub struct MidiRouter {
    /// Keys physically down.
    held: u128,
    /// Keys released while the pedal is down.
    pending: u128,
    sustain: bool,
}

fn bit(note: u8) -> u128 {
    if note < 128 { 1u128 << note } else { 0 }
}

/// Note-off for every key in `set`, in ascending order (Python's `sorted`).
fn release(set: u128, sink: &mut impl RouterSink) {
    let mut s = set;
    while s != 0 {
        let n = s.trailing_zeros() as u8;
        sink.note_off(n);
        s &= s - 1;
    }
}

impl MidiRouter {
    pub fn new() -> MidiRouter {
        MidiRouter::default()
    }

    /// Release every note this router holds (keys and pedal) and lift the pedal.
    /// Call after the port changes or closes so nothing hangs.
    pub fn reset(&mut self, sink: &mut impl RouterSink) {
        release(self.held | self.pending, sink);
        self.held = 0;
        self.pending = 0;
        self.sustain = false;
    }

    pub fn handle(&mut self, e: &MidiEvent, sink: &mut impl RouterSink) {
        match e.kind {
            MidiKind::NoteOn => {
                self.held |= bit(e.note);
                self.pending &= !bit(e.note); // a re-tap cancels its pending off
                sink.note_on(e.note, e.velocity);
            }
            MidiKind::NoteOff => {
                self.held &= !bit(e.note);
                if self.sustain {
                    self.pending |= bit(e.note);
                } else {
                    sink.note_off(e.note);
                }
            }
            MidiKind::Bend => sink.set_control(Control::Bend, e.bend),
            MidiKind::Cc => self.cc(e, sink),
        }
    }

    fn cc(&mut self, e: &MidiEvent, sink: &mut impl RouterSink) {
        match e.cc {
            // Unipolar: the wheel at rest is 0.
            1 => sink.set_control(Control::ModWheel, f64::from(e.value) / 127.0),
            64 => {
                self.sustain = e.value >= 64;
                if !self.sustain {
                    self.flush_pending(sink);
                }
            }
            123 => {
                // All notes off.
                self.flush_pending(sink);
                release(self.held, sink);
                self.held = 0;
            }
            120 => {
                // All sound off.
                self.held = 0;
                self.pending = 0;
                sink.panic();
            }
            _ => sink.other_cc(e.cc, e.value, e.channel),
        }
    }

    fn flush_pending(&mut self, sink: &mut impl RouterSink) {
        release(self.pending, sink);
        self.pending = 0;
    }
}
