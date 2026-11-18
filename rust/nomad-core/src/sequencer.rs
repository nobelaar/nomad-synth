//! 16-step chord sequencer: an event source for one synth. Ported from
//! `nomad/engine/sequencer.py` (the specification).
//!
//! It never produces audio: on each clock step it plays the step's notes through
//! a [`NoteSink`] (the synth's voice manager) and releases them after
//! `gate` x step length. It does not own the clock or the live keys: both are
//! passed on every call.
//!
//! shortcut: a sequencer note-off is skipped when the same key is held live, so
//! the player's key is never cut; a voice-origin tag would be the full fix.

use crate::clock::{Clock, FAR, STEPS, StepGrid, StepTracker};
use crate::voice_manager::VoiceManager;

/// Notes a step holds besides `note`: a chord is up to 6 voices.
pub const EXTRA: usize = 5;
/// Empty chord slot.
pub const NONE: u8 = 255;

/// Where the sequencer plays its notes.
pub trait NoteSink {
    fn note_on(&mut self, note: i64, velocity: f64);
    fn note_off(&mut self, note: i64);
}

impl NoteSink for VoiceManager {
    fn note_on(&mut self, note: i64, velocity: f64) {
        VoiceManager::note_on(self, note, velocity);
    }
    fn note_off(&mut self, note: i64) {
        VoiceManager::note_off(self, note);
    }
}

pub struct StepSequencer {
    pub on: [u8; STEPS],
    pub note: [u8; STEPS],
    pub vel: [u8; STEPS],
    /// Chord notes beyond `note` (`NONE` = empty).
    pub extra: [[u8; EXTRA]; STEPS],
    pub grid: StepGrid,
    pub enabled: bool,
    pub rec: bool,
    /// Step-record cursor.
    pub rec_step: usize,
    tracker: StepTracker,
    gate: f64,
    sr: f64,
    /// Notes the sequencer is sounding.
    held: [u8; EXTRA + 1],
    nheld: usize,
    left: i64,
    /// Step record: keys of the current chord still down.
    rec_down: usize,
    /// Real-time record: absolute step the last note went to (-1 = none).
    rt_last: i64,
}

impl StepSequencer {
    pub fn new(sample_rate: f64) -> StepSequencer {
        StepSequencer {
            on: [0; STEPS],
            note: [60; STEPS],
            vel: [100; STEPS],
            extra: [[NONE; EXTRA]; STEPS],
            grid: StepGrid::new(0.25, STEPS, 0.0),
            enabled: false,
            rec: false,
            rec_step: 0,
            tracker: StepTracker::new(),
            gate: 0.5,
            sr: sample_rate,
            held: [NONE; EXTRA + 1],
            nheld: 0,
            left: FAR,
            rec_down: 0,
            rt_last: -1,
        }
    }

    pub fn gate(&self) -> f64 {
        self.gate
    }

    /// Clamp to [0.05, 1.0]; non-finite -> 0.5.
    pub fn set_gate(&mut self, v: f64) {
        self.gate = if v.is_finite() {
            v.clamp(0.05, 1.0)
        } else {
            0.5
        };
    }

    /// Note clamped to [0, 127], velocity to [1, 127]; `on = false` clears the chord.
    pub fn set_step(&mut self, step: usize, on: bool, note: Option<i64>, vel: Option<i64>) {
        if step >= STEPS {
            return;
        }
        if let Some(n) = note {
            self.note[step] = n.clamp(0, 127) as u8;
        }
        if let Some(v) = vel {
            self.vel[step] = v.clamp(1, 127) as u8;
        }
        self.on[step] = u8::from(on);
        if !on {
            self.extra[step] = [NONE; EXTRA];
        }
    }

    /// The step's notes, `note` first (allocates: tests and GUI only).
    pub fn chord(&self, step: usize) -> Vec<i32> {
        std::iter::once(self.note[step])
            .chain(self.extra[step].iter().copied().filter(|&n| n != NONE))
            .map(i32::from)
            .collect()
    }

    /// Add `n` to the step's chord (no-op when full or already there).
    pub fn add_note(&mut self, step: usize, n: i64) {
        if step >= STEPS {
            return;
        }
        let n = n.clamp(0, 127) as u8;
        if n == self.note[step] || self.extra[step].contains(&n) {
            return;
        }
        if let Some(slot) = self.extra[step].iter_mut().find(|x| **x == NONE) {
            *slot = n;
        }
    }

    /// Move the whole chord, clamped so no note leaves 0..127.
    pub fn transpose(&mut self, step: usize, delta: i64) {
        if step >= STEPS {
            return;
        }
        let notes = self.extra[step].iter().copied().filter(|&n| n != NONE);
        let lo = notes.clone().fold(self.note[step], u8::min) as i64;
        let hi = notes.fold(self.note[step], u8::max) as i64;
        let delta = (-lo).max(delta.min(127 - hi));
        self.note[step] = (self.note[step] as i64 + delta) as u8;
        for x in self.extra[step].iter_mut().filter(|x| **x != NONE) {
            *x = (*x as i64 + delta) as u8;
        }
    }

    pub fn clear(&mut self) {
        self.on = [0; STEPS];
        self.extra = [[NONE; EXTRA]; STEPS];
    }

    fn next_rec_step(&self) -> usize {
        (self.rec_step + 1) % self.grid.length.max(1)
    }

    /// Clock running: write at the nearest step (real time). Stopped: write at the
    /// cursor; the first key rewrites the step, the next ones add to the chord.
    pub fn record(&mut self, note: i64, vel: i64, clock: &Clock) {
        if clock.running {
            self.rec_down = 0; // the clock started while keys were down
            let (g, pos) = (&self.grid, clock.pos);
            let mut k = g.step_at(pos);
            if pos - g.step_start(k) > g.step_start(k + 1) - pos {
                k += 1;
            }
            let step = g.pattern_step(k);
            if k == self.rt_last {
                self.add_note(step, note); // same step as the last note: a chord
            } else {
                self.set_step(step, true, Some(note), Some(vel.max(1)));
                self.clear_extra(step);
                self.rt_last = k;
            }
        } else {
            self.rt_last = -1;
            let step = self.rec_step;
            if self.rec_down == 0 {
                self.set_step(step, true, Some(note), Some(vel.max(1)));
                self.clear_extra(step);
            } else {
                self.add_note(step, note);
            }
            self.rec_down += 1;
        }
    }

    fn clear_extra(&mut self, step: usize) {
        if let Some(e) = self.extra.get_mut(step) {
            *e = [NONE; EXTRA];
        }
    }

    /// A key was released: when the last key of a chord comes up, the cursor advances.
    pub fn record_off(&mut self, clock: &Clock) {
        if self.rec_down > 0 && !clock.running {
            self.rec_down -= 1;
            if self.rec_down == 0 {
                self.rec_step = self.next_rec_step();
            }
        }
    }

    pub fn arm(&mut self, on: bool) {
        self.rec = on;
        self.rec_step = 0;
        self.rec_down = 0;
        self.rt_last = -1;
    }

    /// Step record only (clock stopped, no key down): blank the step and advance.
    pub fn rest(&mut self, clock: &Clock) {
        if !clock.running && self.rec_down == 0 {
            self.set_step(self.rec_step, false, None, None);
            self.rec_step = self.next_rec_step();
        }
    }

    pub fn position(&self, clock: &Clock) -> usize {
        if clock.running {
            self.grid.pattern_step(self.grid.step_at(clock.pos))
        } else {
            0
        }
    }

    pub fn on_step(
        &mut self,
        pos: usize,
        clock: &Clock,
        sink: &mut impl NoteSink,
        live: &[bool; 128],
    ) {
        if !self.enabled {
            if self.nheld > 0 {
                self.release(sink, live);
            }
            return;
        }
        self.off(sink, live);
        if self.on[pos] != 0 {
            let vel = f64::from(self.vel[pos]) / 127.0;
            self.sound(i64::from(self.note[pos]), vel, sink);
            for i in 0..EXTRA {
                let n = self.extra[pos][i];
                if n != NONE {
                    self.sound(i64::from(n), vel, sink);
                }
            }
            let frames = self.gate * self.grid.step_beats(pos as i64) * 60.0 * self.sr / clock.bpm;
            self.left = (frames as i64).max(1);
        }
    }

    pub fn poll(
        &mut self,
        clock: &Clock,
        clock_id: u64,
        sink: &mut impl NoteSink,
        live: &[bool; 128],
    ) {
        let step = self.tracker.poll(&self.grid, clock, clock_id);
        if !clock.running {
            self.release(sink, live); // clock stopped: never leave a hung note
        } else if let Some(s) = step {
            self.on_step(s, clock, sink, live);
        }
    }

    pub fn frames_to_event(&self, clock: &Clock) -> i64 {
        let n = self.frames_to_off();
        if self.enabled {
            n.min(self.tracker.frames_to_event(&self.grid, clock))
        } else {
            n
        }
    }

    fn frames_to_off(&self) -> i64 {
        if self.nheld > 0 { self.left } else { FAR }
    }

    pub fn advance(&mut self, n: i64, sink: &mut impl NoteSink, live: &[bool; 128]) {
        if self.nheld == 0 {
            return;
        }
        if !self.enabled {
            self.release(sink, live);
            return;
        }
        self.left -= n;
        if self.left <= 0 {
            self.off(sink, live);
        }
    }

    fn sound(&mut self, n: i64, vel: f64, sink: &mut impl NoteSink) {
        sink.note_on(n, vel);
        if self.nheld < self.held.len() {
            self.held[self.nheld] = n as u8;
            self.nheld += 1;
        }
    }

    fn off(&mut self, sink: &mut impl NoteSink, live: &[bool; 128]) {
        if self.nheld > 0 {
            for &n in &self.held[..self.nheld] {
                if !live[n as usize] {
                    sink.note_off(i64::from(n));
                }
            }
            self.nheld = 0;
            self.left = FAR;
        }
    }

    /// Release the sounding notes (honours live-held keys).
    pub fn release(&mut self, sink: &mut impl NoteSink, live: &[bool; 128]) {
        self.off(sink, live);
    }
}
