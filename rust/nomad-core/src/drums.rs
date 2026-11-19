//! Drum machine runtime: six synthesized one-shots, a 16-step pattern and an
//! allocation-free mix. Ported from `DrumMachine` in `nomad/engine/drums.py`
//! (the specification). The clock is passed in, like the synth's.
//!
//! The regeneration throttle (`REGEN_COOLDOWN_S`, `dirty`, `flush`) is GUI-side
//! (M3): `set_value` only reports that a track needs a new shot, and the caller
//! renders it off the audio thread and hands it over with `set_bank`.

use crate::clock::{Clock, STEPS, StepGrid, StepTracker};
use crate::drum_synth::render_one_shot;
use crate::params::DRUM_PARAMS;

pub const ACCENT_GAIN: f64 = 1.0;
pub const NORMAL_GAIN: f64 = 0.6;
pub const FADE_MS: f64 = 5.0;
const TRACKS: usize = 6;
const CAP: usize = 2048;
const BPM: usize = 18;
const SWING: usize = 19;

/// What a `set_value` asks of the caller.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ValueEffect {
    None,
    /// Re-synthesize this track's one-shot (off the audio thread) and `set_bank` it.
    Regenerate(usize),
    /// Send this tempo to the machine's clock.
    Bpm(f64),
}

/// Which buffer a track's playing voice reads.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Src {
    None,
    Bank,
    /// The shot that was playing when `set_bank` replaced it (Python keeps the old array).
    Old,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    src: Src,
    off: usize,
    gain: f64,
}

pub struct DrumMachine {
    pub steps: [[u8; STEPS]; TRACKS],
    pub grid: StepGrid,
    /// Drum parameter values, indexed like `DRUM_PARAMS`.
    pub values: [f64; 20],
    bank: [Vec<f32>; TRACKS],
    old: [Vec<f32>; TRACKS],
    active: [Active; TRACKS],
    tracker: StepTracker,
    fade_len: i64,
    fade_left: i64,
    mix: Vec<f64>,
}

impl DrumMachine {
    /// Synthesizes the six default shots (allocates: not for the audio thread).
    pub fn new(sample_rate: f64) -> DrumMachine {
        let values: [f64; 20] = std::array::from_fn(|i| DRUM_PARAMS[i].default);
        let bank = std::array::from_fn(|t| {
            render_one_shot(t, sample_rate, values[3 * t], values[3 * t + 1])
        });
        let mut dm = DrumMachine {
            steps: [[0; STEPS]; TRACKS],
            grid: StepGrid::new(0.25, STEPS, values[SWING]),
            values,
            bank,
            old: std::array::from_fn(|_| Vec::new()),
            active: [Active {
                src: Src::None,
                off: 0,
                gain: 0.0,
            }; TRACKS],
            tracker: StepTracker::new(),
            fade_len: ((FADE_MS / 1000.0 * sample_rate) as i64).max(1),
            fade_left: 0,
            mix: vec![0.0; CAP],
        };
        dm.default_pattern();
        dm
    }

    fn default_pattern(&mut self) {
        self.clear_pattern();
        for step in [0, 4, 8, 12] {
            self.steps[0][step] = 1;
        }
        self.steps[0][0] = 2;
        for step in [4, 12] {
            self.steps[1][step] = 1;
        }
        for step in (0..STEPS).step_by(2) {
            self.steps[4][step] = 1;
        }
    }

    pub fn clear_pattern(&mut self) {
        self.steps = [[0; STEPS]; TRACKS];
    }

    /// Level clamped to 0..=2 (off, normal, accent); bad track or step is ignored.
    pub fn set_step(&mut self, track: usize, step: usize, level: u8) {
        if track < TRACKS && step < STEPS {
            self.steps[track][step] = level.min(2);
        }
    }

    /// Clamp through the drum registry and store; unknown ids are ignored.
    pub fn set_value(&mut self, id: usize, value: f64) -> ValueEffect {
        let Some(spec) = DRUM_PARAMS.get(id) else {
            return ValueEffect::None;
        };
        let v = spec.clamp(value);
        self.values[id] = v;
        match id {
            BPM => ValueEffect::Bpm(v),
            SWING => {
                self.grid.swing = v;
                ValueEffect::None
            }
            _ if id % 3 < 2 => ValueEffect::Regenerate(id / 3), // tune or decay
            _ => ValueEffect::None,
        }
    }

    /// Swap in a new one-shot for `track`. A voice still playing the old one keeps
    /// it; the buffer nobody needs any more is returned for the caller to free
    /// (no deallocation here). A bad track hands `shot` straight back.
    pub fn set_bank(&mut self, track: usize, shot: Vec<f32>) -> Vec<f32> {
        if track >= TRACKS {
            return shot;
        }
        let previous = std::mem::replace(&mut self.bank[track], shot);
        if self.active[track].src == Src::Bank {
            self.active[track].src = Src::Old;
            std::mem::replace(&mut self.old[track], previous)
        } else {
            previous
        }
    }

    /// Start `track`'s shot from the top; non-finite velocity counts as 1.
    pub fn trigger(&mut self, track: usize, velocity: f64) {
        if track >= TRACKS {
            return;
        }
        let v = if velocity.is_finite() { velocity } else { 1.0 };
        self.active[track] = Active {
            src: Src::Bank,
            off: 0,
            gain: v.clamp(0.0, 1.0) * self.values[3 * track + 2],
        };
    }

    /// Stop the clock and fade everything out over 5 ms.
    pub fn stop_all(&mut self, clock: &mut Clock) {
        clock.stop();
        self.fade_left = self.fade_len;
    }

    pub fn playing(&self, clock: &Clock) -> bool {
        clock.running
    }

    pub fn position(&self, clock: &Clock) -> usize {
        if clock.running {
            self.grid.pattern_step(self.grid.step_at(clock.pos))
        } else {
            0
        }
    }

    /// Nothing to render: clock stopped, no fade, no shot still sounding.
    pub fn is_idle(&self, clock: &Clock) -> bool {
        !clock.running
            && self.fade_left == 0
            && self
                .active
                .iter()
                .enumerate()
                .all(|(i, a)| a.src == Src::None || a.off >= self.buf(i, a.src).len())
    }

    fn buf(&self, track: usize, src: Src) -> &[f32] {
        match src {
            Src::Bank => &self.bank[track],
            Src::Old => &self.old[track],
            Src::None => &[],
        }
    }

    pub fn on_step(&mut self, step: usize) {
        for i in 0..TRACKS {
            let level = self.steps[i][step];
            if level != 0 {
                self.trigger(i, if level == 2 { ACCENT_GAIN } else { NORMAL_GAIN });
            }
        }
    }

    pub fn poll(&mut self, clock: &Clock, clock_id: u64) {
        if let Some(step) = self.tracker.poll(&self.grid, clock, clock_id) {
            self.on_step(step);
        }
    }

    pub fn frames_to_event(&self, clock: &Clock) -> i64 {
        let n = self.tracker.frames_to_event(&self.grid, clock);
        if self.fade_left > 0 {
            n.min(self.fade_left)
        } else {
            n
        }
    }

    /// ADD one chunk of the mix into `out`. The gain multiply runs in `f32`, like
    /// numpy's `float32 * Python float` (NEP 50), then accumulates in `f64`.
    pub fn render_chunk(&mut self, out: &mut [f64]) {
        let chunk = out.len();
        if chunk > self.mix.len() {
            // shortcut: chunks above 2048 grow the scratch once (allocates); the engine uses <= 256.
            self.mix.resize(chunk, 0.0);
        }
        let mix = &mut self.mix[..chunk];
        mix.fill(0.0);
        for i in 0..TRACKS {
            let Active { src, off, gain } = self.active[i];
            let arr: &[f32] = match src {
                Src::Bank => &self.bank[i],
                Src::Old => &self.old[i],
                Src::None => continue,
            };
            if off >= arr.len() || gain == 0.0 {
                continue;
            }
            let seg = chunk.min(arr.len() - off);
            let g = gain as f32;
            for (m, &s) in mix[..seg].iter_mut().zip(&arr[off..off + seg]) {
                *m += f64::from(s * g);
            }
            self.active[i].off = off + seg;
        }
        if self.fade_left > 0 {
            let g0 = (self.fade_left as f64 / self.fade_len as f64).min(1.0);
            let left = (self.fade_left - chunk as i64).max(0);
            let g1 = left as f64 / self.fade_len as f64;
            let step = (g1 - g0) / chunk as f64;
            for (j, m) in mix.iter_mut().enumerate() {
                *m *= j as f64 * step + g0;
            }
            self.fade_left = left;
            if left == 0 {
                self.active.fill(Active {
                    src: Src::None,
                    off: 0,
                    gain: 0.0,
                });
            }
        }
        for (o, m) in out.iter_mut().zip(mix.iter()) {
            *o += m;
        }
    }
}
