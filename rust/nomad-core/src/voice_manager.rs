//! Voice allocation and stealing over a fixed pool, with glide and mono legato.
//! Ported from `nomad/engine/voice_manager.py` (the specification).
//!
//! A note already sounding is retriggered on its own voice; else a free voice if
//! fewer than `polyphony` are active; else steal the oldest voice in release, or
//! the oldest held one. Nothing here allocates after `new` (except `active_notes`).

use crate::mods::Mods;
use crate::params::ParamValues;
use crate::voice::Voice;

pub struct VoiceManager {
    /// Read only: set through `set_polyphony`.
    pub polyphony: usize,
    /// Read only, in ms: set through `set_glide`. > 0 enables glide and mono legato.
    pub glide: f64,
    voices: Vec<Voice>,
    held: Vec<bool>,
    stamp: Vec<u64>,
    clock: u64,
    /// Last note played: where a fresh voice starts its glide.
    last: Option<i32>,
}

/// Python's `_note`: only keys 0..=127 are valid.
fn valid_note(n: i64) -> Option<i32> {
    (0..=127).contains(&n).then_some(n as i32)
}

impl VoiceManager {
    pub fn new(sample_rate: f64, max_voices: usize) -> VoiceManager {
        let n = max_voices.max(1);
        VoiceManager {
            polyphony: n,
            glide: 0.0,
            voices: (0..n).map(|_| Voice::new(sample_rate)).collect(),
            held: vec![false; n],
            stamp: vec![0; n],
            clock: 0,
            last: None,
        }
    }

    fn is_held(&self, i: usize) -> bool {
        self.held[i] && self.voices[i].is_active()
    }

    /// Oldest (smallest stamp) voice index among those `pick` accepts.
    fn oldest(&self, pick: impl Fn(usize) -> bool) -> Option<usize> {
        (0..self.voices.len())
            .filter(|&i| pick(i))
            .min_by_key(|&i| self.stamp[i])
    }

    fn release(&mut self, i: usize) {
        self.held[i] = false;
        self.voices[i].note_off();
    }

    /// Clamp to 1..=max; extra held voices are released, oldest first.
    pub fn set_polyphony(&mut self, n: i64) {
        self.polyphony = n.clamp(1, self.voices.len() as i64) as usize;
        let held = (0..self.voices.len()).filter(|&i| self.is_held(i)).count();
        for _ in 0..held.saturating_sub(self.polyphony) {
            if let Some(i) = self.oldest(|i| self.is_held(i)) {
                self.release(i);
            }
        }
    }

    /// `NaN`, infinities and values <= 0 disable glide.
    pub fn set_glide(&mut self, ms: f64) {
        self.glide = if ms.is_finite() && ms > 0.0 { ms } else { 0.0 };
    }

    pub fn note_on(&mut self, note: i64, velocity: f64) {
        let Some(note) = valid_note(note) else {
            return;
        };
        self.clock += 1;
        let clock = self.clock;
        if self.glide > 0.0 && self.polyphony == 1 {
            // Mono legato: the held voice glides to the new key, envelopes keep running.
            // shortcut: last-note rule without a note stack, so releasing the newest key
            // of an overlapping pair does not return to the older one; add a stack if wanted.
            if let Some(i) = (0..self.voices.len()).find(|&i| self.is_held(i)) {
                if self.voices[i].note != note {
                    self.voices[i].set_target(note);
                }
                self.stamp[i] = clock;
                self.voices[i].age = clock;
                self.last = Some(note);
                return;
            }
        }
        let active = |i: usize| self.voices[i].is_active();
        let n_active = (0..self.voices.len()).filter(|&i| active(i)).count();
        let (i, steal) = if let Some(i) = self.oldest(|i| active(i) && self.voices[i].note == note)
        {
            (i, true)
        } else if n_active < self.polyphony && n_active < self.voices.len() {
            ((0..self.voices.len()).find(|&j| !active(j)).unwrap(), false)
        } else {
            let i = self
                .oldest(|j| active(j) && !self.held[j])
                .or_else(|| self.oldest(active))
                .unwrap();
            (i, true)
        };
        let start = self.last.map(f64::from);
        let v = &mut self.voices[i];
        if steal {
            v.begin_steal(note, velocity);
        } else {
            v.note_on(note, velocity, start);
        }
        v.age = clock;
        self.last = Some(note);
        self.held[i] = true;
        self.stamp[i] = clock;
    }

    pub fn note_off(&mut self, note: i64) {
        let Some(note) = valid_note(note) else {
            return;
        };
        for i in 0..self.voices.len() {
            if self.is_held(i) && self.voices[i].note == note {
                self.release(i);
            }
        }
    }

    /// Held keys in voice order (allocates: tests and GUI only).
    pub fn active_notes(&self) -> Vec<i32> {
        let mut buf = [0i32; 128];
        let n = self.fill_active_notes(&mut buf);
        buf[..n].to_vec()
    }

    /// Held keys as a 128-bit mask (no allocation; for the UI mirrors).
    pub fn held_mask(&self) -> u128 {
        (0..self.voices.len())
            .filter(|&i| self.is_held(i))
            .map(|i| self.voices[i].note)
            .filter(|n| (0..128).contains(n))
            .fold(0u128, |m, n| m | (1u128 << n))
    }

    /// Voices currently sounding (held, releasing or fading).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Held keys in voice order, written into `out`; returns how many.
    pub fn fill_active_notes(&self, out: &mut [i32; 128]) -> usize {
        let mut n = 0;
        for i in 0..self.voices.len() {
            if self.is_held(i) && n < out.len() {
                out[n] = self.voices[i].note;
                n += 1;
            }
        }
        n
    }

    /// Release everything: each active voice fades to silence over the steal fade.
    pub fn panic(&mut self) {
        for (v, held) in self.voices.iter_mut().zip(self.held.iter_mut()) {
            if v.is_active() {
                let note = v.note;
                v.begin_steal(note, 0.0);
                v.note_off(); // cancels the retrigger; the fade ends in silence
            }
            *held = false;
        }
    }

    /// Each active voice adds into `out` with the mods `resolve(voice, mods)` returns.
    pub fn render<F: FnMut(&Voice, &Mods) -> Mods>(
        &mut self,
        out: &mut [f64],
        p: &ParamValues,
        mods: &Mods,
        mut resolve: F,
    ) {
        for v in self.voices.iter_mut().filter(|v| v.is_active()) {
            let m = resolve(v, mods);
            v.render(out, p, &m);
        }
    }
}
