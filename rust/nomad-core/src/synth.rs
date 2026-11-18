//! DSP core of one Synth instance: voices, smoothed parameters, two LFOs, the
//! modulation matrix, the bend/modwheel controls and the step sequencer. Ported
//! from `SynthInstrument` in `nomad/engine/instrument.py` (the specification),
//! without the command queue (M2): notes and parameters are applied directly,
//! in call order, which matches Python draining its queue before every block.
//! The clock is not owned here (topology is M1-D): it is passed in.

use crate::clock::Clock;
use crate::lfo::Lfo;
use crate::modmatrix::{ModMatrix, NUM_SLOTS, Sources};
use crate::mods::Mods;
use crate::params::{ParamValues, SYNTH_PARAMS, default_values, pid};
use crate::sequencer::StepSequencer;
use crate::smoother::Smoother;
use crate::voice_manager::VoiceManager;

pub const CONTROL_BLOCK: usize = 256;
/// Semitones per unit of matrix pitch modulation.
pub const PITCH_SEMITONES: f64 = 12.0;
const CTRL_SMOOTH_MS: f64 = 10.0;
const IMMEDIATE_SUFFIX: [&str; 10] = [
    "_wave", "_mode", "_sync", "_phase", "_src", "_dst", "_attack", "_decay", "_sustain",
    "_release",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Control {
    Bend,
    ModWheel,
}

/// Whether parameter `id` is smoothed (Python's `_SMOOTHED`) rather than applied at once.
pub fn is_smoothed(id: usize) -> bool {
    let Some(spec) = SYNTH_PARAMS.get(id) else {
        return false;
    };
    let n = spec.name;
    n != "polyphony"
        && n != "master_volume"
        && !IMMEDIATE_SUFFIX.iter().any(|s| n.ends_with(s))
        && !(n.starts_with("mod") && n.ends_with("_amt"))
}

pub struct Synth {
    pub sequencer: StepSequencer,
    vm: VoiceManager,
    matrix: ModMatrix,
    lfos: [Lfo; 2],
    /// Caller-side targets (what `set_param` stored).
    values: ParamValues,
    /// What the voices read (smoothed values move towards `values`).
    p: ParamValues,
    smoothed: [bool; pid::COUNT],
    smoothers: [Smoother; pid::COUNT],
    bend: Smoother,
    modwheel: Smoother,
    slots: [(usize, usize, f64); NUM_SLOTS],
    src: Sources,
    live: [bool; 128],
    sr: f64,
}

impl Synth {
    pub fn new(sample_rate: f64, max_voices: usize) -> Synth {
        let sr = sample_rate;
        Synth {
            sequencer: StepSequencer::new(sr),
            vm: VoiceManager::new(sr, max_voices),
            matrix: ModMatrix::new(),
            lfos: [Lfo::new(sr, 1), Lfo::new(sr, 2)],
            values: default_values(),
            p: default_values(),
            smoothed: std::array::from_fn(is_smoothed),
            smoothers: std::array::from_fn(|i| {
                let s = &SYNTH_PARAMS[i];
                Smoother::new(s.to_norm(s.default), sr, s.smooth_ms)
            }),
            bend: Smoother::new(0.0, sr, CTRL_SMOOTH_MS),
            modwheel: Smoother::new(0.0, sr, CTRL_SMOOTH_MS),
            slots: [(0, 0, 0.0); NUM_SLOTS],
            src: Sources::default(),
            live: [false; 128],
            sr,
        }
    }

    /// A played key; while REC is armed it is also recorded.
    pub fn note_on(&mut self, note: i64, velocity: f64, clock: &Clock) {
        self.vm.note_on(note, velocity);
        if self.sequencer.rec {
            self.sequencer
                .record(note, (velocity * 127.0) as i64, clock);
        }
        if let Ok(n @ 0..=127) = usize::try_from(note) {
            self.live[n] = true;
        }
    }

    pub fn note_off(&mut self, note: i64, clock: &Clock) {
        self.vm.note_off(note);
        if self.sequencer.rec {
            self.sequencer.record_off(clock);
        }
        if let Ok(n @ 0..=127) = usize::try_from(note) {
            self.live[n] = false;
        }
    }

    /// Clamp through the registry and apply. `master_volume` (owned by the mixer)
    /// and unknown ids are ignored (Python raises `KeyError`).
    pub fn set_param(&mut self, id: usize, value: f64) {
        if id >= pid::COUNT || id == pid::MASTER_VOLUME {
            return;
        }
        let spec = &SYNTH_PARAMS[id];
        let v = spec.clamp(value);
        self.values[id] = v;
        if self.smoothed[id] {
            self.smoothers[id].set(spec.to_norm(v));
        } else {
            self.p[id] = v;
        }
        if id == pid::POLYPHONY {
            self.vm.set_polyphony(v as i64);
        } else if id == pid::GLIDE {
            self.vm.set_glide(v);
        } else if (pid::MOD0_SRC..pid::MOD0_SRC + 3 * NUM_SLOTS).contains(&id) {
            let (i, field) = ((id - pid::MOD0_SRC) / 3, (id - pid::MOD0_SRC) % 3);
            let slot = &mut self.slots[i];
            // Python's round() is round-half-to-even.
            match field {
                0 => slot.0 = v.round_ties_even() as usize,
                1 => slot.1 = v.round_ties_even() as usize,
                _ => slot.2 = v,
            }
            // In range after the registry clamp, so this cannot fail.
            let _ = self.matrix.set_slot(i, slot.0, slot.1, slot.2);
        }
    }

    /// Clamp to [-1, 1]; `NaN` (any non-finite) -> 0.
    pub fn set_control(&mut self, ctl: Control, value: f64) {
        let v = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        match ctl {
            Control::Bend => self.bend.set(v),
            Control::ModWheel => self.modwheel.set(v),
        }
    }

    /// Disarm REC, forget live keys, fade every voice out and release the sequencer note.
    pub fn panic(&mut self) {
        self.sequencer.arm(false);
        self.live = [false; 128];
        self.vm.panic();
        self.sequencer.release(&mut self.vm, &self.live);
    }

    /// Arm or disarm REC (the cursor restarts at step 1).
    pub fn arm_rec(&mut self, on: bool) {
        self.sequencer.arm(on);
    }

    /// Step-record rest (only with the clock stopped).
    pub fn rest(&mut self, clock: &Clock) {
        self.sequencer.rest(clock);
    }

    /// Clock hook: fire the sequencer step due at `clock.pos`, if any.
    pub fn poll_sequencer(&mut self, clock: &Clock, clock_id: u64) {
        self.sequencer
            .poll(clock, clock_id, &mut self.vm, &self.live);
    }

    /// Frames until the next sequencer event (step or note-off).
    pub fn frames_to_event(&self, clock: &Clock) -> i64 {
        self.sequencer.frames_to_event(clock)
    }

    /// Clock hook: `n` frames went by (gate countdown).
    pub fn advance_sequencer(&mut self, n: i64) {
        self.sequencer.advance(n, &mut self.vm, &self.live);
    }

    /// Held keys (voice manager), as a 128-bit mask.
    pub fn held_mask(&self) -> u128 {
        self.vm.held_mask()
    }

    /// Voices currently sounding.
    pub fn active_voices(&self) -> usize {
        self.vm.active_voices()
    }

    /// Parameter targets as last set (not the smoothed values).
    pub fn values(&self) -> &ParamValues {
        &self.values
    }

    /// Key held through `note_on`/`note_off` (not by the sequencer).
    pub fn is_live(&self, note: usize) -> bool {
        self.live.get(note).copied().unwrap_or(false)
    }

    /// Advance control state by one block and ADD the voices into `out`
    /// (`out.len() <= CONTROL_BLOCK` in the engine).
    pub fn render_block(&mut self, out: &mut [f64]) {
        let n = out.len();
        for (id, sm) in self.smoothers.iter_mut().enumerate() {
            if self.smoothed[id] && sm.value != sm.target {
                self.p[id] = SYNTH_PARAMS[id].from_norm(sm.advance(n as i64));
            }
        }
        for sm in [&mut self.bend, &mut self.modwheel] {
            if sm.value != sm.target {
                sm.advance(n as i64);
            }
        }
        let p = &self.p;
        let sec = n as f64 / self.sr;
        let lfo_ids = [
            (
                pid::LFO1_RATE,
                pid::LFO1_WAVE,
                pid::LFO1_DEPTH,
                pid::LFO1_PHASE,
            ),
            (
                pid::LFO2_RATE,
                pid::LFO2_WAVE,
                pid::LFO2_DEPTH,
                pid::LFO2_PHASE,
            ),
        ];
        let mut lfo_out = [0.0; 2];
        for ((lfo, (rate, wave, depth, phase)), v) in
            self.lfos.iter_mut().zip(lfo_ids).zip(&mut lfo_out)
        {
            // The registry keeps the wave in 0..=4, so set cannot fail.
            let _ = lfo.set(p[rate], p[wave] as i64, p[depth], p[phase]);
            *v = lfo.advance(sec);
        }
        self.src.lfo1 = lfo_out[0];
        self.src.lfo2 = lfo_out[1];
        self.src.modwheel = self.modwheel.value;
        let mods = Mods {
            bend: self.bend.value * p[pid::BEND_RANGE],
            ..Mods::default()
        };
        let (matrix, src) = (&mut self.matrix, &mut self.src);
        self.vm.render(out, p, &mods, |voice, m| {
            src.velocity = voice.velocity;
            src.env2 = voice.env2_level;
            let r = matrix.resolve(src);
            Mods {
                pitch: r.pitch * PITCH_SEMITONES,
                pitch1: r.pitch1 * PITCH_SEMITONES,
                pitch2: r.pitch2 * PITCH_SEMITONES,
                amp: r.amp,
                cutoff: r.cutoff,
                pw1: r.pw1,
                pw2: r.pw2,
                bend: m.bend, // bend is set once per block, not per voice
            }
        });
    }
}
