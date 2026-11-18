//! The engine: owns every instrument instance and its clock, mixes them (level,
//! mute, master volume, tanh limiter) and drives their clocks. Ported from
//! `Engine` in `nomad/engine/engine.py`, `InstrumentBase` in `instance.py` and
//! (for the graph) `InstrumentManager`/`Topology` (the specification).
//!
//! Commands are direct method calls applied immediately, in call order (the
//! command queue towards the audio thread is M2). Python applies a published
//! topology before each instance's own commands at the next render; callers that
//! mix both in one gap get call order here.

use crate::clock::Clock;
use crate::drum_synth::render_one_shot;
use crate::drums::{DrumMachine, ValueEffect};
use crate::params::{SYNTH_PARAMS, pid};
use crate::smoother::Smoother;
use crate::synth::{CONTROL_BLOCK, Synth};

pub type InstrumentId = u32;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Synth,
    Drums,
}

enum Body {
    Synth(Box<Synth>),
    Drums(Box<DrumMachine>),
}

struct Inst {
    id: InstrumentId,
    name: String,
    body: Body,
    clock: Clock,
    /// `None` = Internal (own clock); `Some(src)` follows `src`'s clock.
    source: Option<InstrumentId>,
    level: f64,
    muted: bool,
    igain: f64,
    closing: bool,
    buf: Vec<f64>,
}

impl Inst {
    fn new(id: InstrumentId, name: String, body: Body, sr: f64) -> Inst {
        let mut clock = Clock::new(sr);
        if let Body::Drums(d) = &body {
            clock.bpm = d.values[18];
        }
        Inst {
            id,
            name,
            body,
            clock,
            source: None,
            level: 1.0,
            muted: false,
            igain: 1.0,
            closing: false,
            buf: vec![0.0; CONTROL_BLOCK],
        }
    }

    fn kind(&self) -> Kind {
        match self.body {
            Body::Synth(_) => Kind::Synth,
            Body::Drums(_) => Kind::Drums,
        }
    }

    fn gain_target(&self) -> f64 {
        if self.muted { 0.0 } else { self.level }
    }

    fn poll(&mut self, clock: &Clock, clock_id: u64) {
        match &mut self.body {
            Body::Synth(s) => s.poll_sequencer(clock, clock_id),
            Body::Drums(d) => d.poll(clock, clock_id),
        }
    }

    fn frames_to_event(&self, clock: &Clock) -> i64 {
        match &self.body {
            Body::Synth(s) => s.frames_to_event(clock),
            Body::Drums(d) => d.frames_to_event(clock),
        }
    }

    fn advance(&mut self, n: i64) {
        if let Body::Synth(s) = &mut self.body {
            s.advance_sequencer(n);
        }
    }
}

pub struct Engine {
    sr: f64,
    /// Mix order: live instances, then the closing ones.
    insts: Vec<Inst>,
    target_synth: InstrumentId,
    master_sm: Smoother,
    master_p: f64,
    master: f64,
    buf: Vec<f64>,
}

impl Engine {
    /// `Synth 01` (id 1) and `Machine Drum 01` (id 2), both Internal at unity level.
    pub fn new(sample_rate: f64, max_voices: usize) -> Engine {
        let sr = sample_rate;
        let spec = &SYNTH_PARAMS[pid::MASTER_VOLUME];
        let insts = vec![
            Inst::new(
                1,
                "Synth 01".into(),
                Body::Synth(Box::new(Synth::new(sr, max_voices))),
                sr,
            ),
            Inst::new(
                2,
                "Machine Drum 01".into(),
                Body::Drums(Box::new(DrumMachine::new(sr))),
                sr,
            ),
        ];
        Engine {
            sr,
            insts,
            target_synth: 1,
            master_sm: Smoother::new(spec.to_norm(spec.default), sr, spec.smooth_ms),
            master_p: spec.default,
            master: spec.default,
            buf: vec![0.0; CONTROL_BLOCK],
        }
    }

    fn index(&self, id: InstrumentId) -> Option<usize> {
        self.insts.iter().position(|i| i.id == id)
    }

    fn inst_mut(&mut self, id: InstrumentId) -> Option<&mut Inst> {
        self.insts.iter_mut().find(|i| i.id == id)
    }

    /// Clock an instance plays from (its source's, if it follows one), plus that clock's id.
    fn eff(&self, i: usize) -> (Clock, u64) {
        let inst = &self.insts[i];
        let owner = inst.source.and_then(|s| self.index(s)).unwrap_or(i);
        (
            self.insts[owner].clock.clone(),
            u64::from(self.insts[owner].id),
        )
    }

    /// Own-clock command (`start`, `stop`, `bpm`): false for unknown ids and followers.
    fn own(&mut self, id: InstrumentId, f: impl FnOnce(&mut Clock)) -> bool {
        match self.inst_mut(id) {
            Some(inst) if inst.source.is_none() => {
                f(&mut inst.clock);
                true
            }
            _ => false,
        }
    }

    pub fn start(&mut self, id: InstrumentId) -> bool {
        self.own(id, Clock::start)
    }

    pub fn stop(&mut self, id: InstrumentId) -> bool {
        self.own(id, Clock::stop)
    }

    /// Clamped to [20, 300]; a non-finite tempo is ignored (still `true`, like Python).
    pub fn set_bpm(&mut self, id: InstrumentId, bpm: f64) -> bool {
        self.own(id, |c| {
            if bpm.is_finite() {
                c.bpm = bpm.clamp(20.0, 300.0);
            }
        })
    }

    pub fn set_mute(&mut self, id: InstrumentId, muted: bool) {
        if let Some(inst) = self.inst_mut(id) {
            inst.muted = muted;
        }
    }

    /// Clamped to [0, 1]; non-finite -> 1.0.
    pub fn set_level(&mut self, id: InstrumentId, level: f64) {
        if let Some(inst) = self.inst_mut(id) {
            inst.level = if level.is_finite() {
                level.clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
    }

    pub fn name(&self, id: InstrumentId) -> Option<&str> {
        self.index(id).map(|i| self.insts[i].name.as_str())
    }

    pub fn level(&self, id: InstrumentId) -> Option<f64> {
        self.index(id).map(|i| self.insts[i].level)
    }

    /// Mix volume, clamped through the registry and smoothed.
    pub fn set_master_volume(&mut self, v: f64) {
        let spec = &SYNTH_PARAMS[pid::MASTER_VOLUME];
        self.master_sm.set(spec.to_norm(spec.clamp(v)));
    }

    /// Synth parameter on the target synth (`master_volume` goes to the mixer).
    pub fn set_param(&mut self, id: usize, value: f64) {
        if id == pid::MASTER_VOLUME {
            return self.set_master_volume(value);
        }
        let target = self.target_synth;
        if let Some(s) = self.synth_mut(target) {
            s.set_param(id, value);
        }
    }

    /// Drum parameter: tempo goes to the machine's clock, tone changes re-synthesize
    /// the shot here (allocates: caller thread only, the GUI throttle is M3).
    pub fn set_drum_value(&mut self, id: InstrumentId, param: usize, value: f64) {
        let sr = self.sr;
        let Some(dm) = self.drums_mut(id) else {
            return;
        };
        match dm.set_value(param, value) {
            ValueEffect::Bpm(b) => {
                self.set_bpm(id, b);
            }
            ValueEffect::Regenerate(t) => {
                let shot = render_one_shot(t, sr, dm.values[3 * t], dm.values[3 * t + 1]);
                dm.set_bank(t, shot);
            }
            ValueEffect::None => {}
        }
    }

    pub fn note_on(&mut self, note: i64, velocity: f64) {
        let Some(i) = self.index(self.target_synth) else {
            return;
        };
        let (clock, _) = self.eff(i);
        if let Body::Synth(s) = &mut self.insts[i].body {
            s.note_on(note, velocity, &clock);
        }
    }

    pub fn note_off(&mut self, note: i64) {
        let Some(i) = self.index(self.target_synth) else {
            return;
        };
        let (clock, _) = self.eff(i);
        if let Body::Synth(s) = &mut self.insts[i].body {
            s.note_off(note, &clock);
        }
    }

    /// Which synth receives `note_on`/`note_off`/`set_param`; false if `id` is not a live synth.
    pub fn set_target_synth(&mut self, id: InstrumentId) -> bool {
        let ok = self
            .index(id)
            .is_some_and(|i| self.insts[i].kind() == Kind::Synth && !self.insts[i].closing);
        if ok {
            self.target_synth = id;
        }
        ok
    }

    pub fn synth_mut(&mut self, id: InstrumentId) -> Option<&mut Synth> {
        match &mut self.inst_mut(id)?.body {
            Body::Synth(s) => Some(s),
            Body::Drums(_) => None,
        }
    }

    pub fn drums_mut(&mut self, id: InstrumentId) -> Option<&mut DrumMachine> {
        match &mut self.inst_mut(id)?.body {
            Body::Drums(d) => Some(d),
            Body::Synth(_) => None,
        }
    }

    /// The instance's own clock.
    pub fn clock(&self, id: InstrumentId) -> Option<&Clock> {
        self.index(id).map(|i| &self.insts[i].clock)
    }

    /// The clock the instance plays from: its source's if it follows one.
    pub fn effective_clock(&self, id: InstrumentId) -> Option<&Clock> {
        let i = self.index(id)?;
        let owner = self.insts[i]
            .source
            .and_then(|s| self.index(s))
            .unwrap_or(i);
        Some(&self.insts[owner].clock)
    }

    /// `[pos, bpm, running]` of every instance's own clock, in mix order (tests).
    pub fn own_clock_states(&self) -> Vec<[f64; 3]> {
        self.insts
            .iter()
            .map(|i| {
                [
                    i.clock.pos,
                    i.clock.bpm,
                    f64::from(u8::from(i.clock.running)),
                ]
            })
            .collect()
    }

    fn poll_all(&mut self) {
        for i in 0..self.insts.len() {
            let (clock, id) = self.eff(i);
            self.insts[i].poll(&clock, id);
        }
    }

    /// Render `out` (written, not added): events are scheduled exactly, each chunk
    /// is mixed in CONTROL_BLOCK slices and every root clock advances once per chunk.
    pub fn render(&mut self, out: &mut [f64]) {
        let total = out.len();
        let mut i = 0;
        while i < total {
            self.poll_all();
            let mut limit = 1i64 << 30;
            for k in 0..self.insts.len() {
                let (clock, _) = self.eff(k);
                limit = limit.min(self.insts[k].frames_to_event(&clock));
            }
            let chunk = (total - i).min(limit as usize);
            self.span(&mut out[i..i + chunk]);
            for inst in self.insts.iter_mut().filter(|x| x.source.is_none()) {
                inst.clock.advance(chunk as i64);
            }
            i += chunk;
        }
        // Events landing exactly on the buffer edge fire now, not one render later.
        self.poll_all();
    }

    /// One event-free chunk, in control-block slices.
    fn span(&mut self, out: &mut [f64]) {
        let chunk = out.len();
        for block in out.chunks_mut(CONTROL_BLOCK) {
            let n = block.len();
            let b = &mut self.buf[..n];
            b.fill(0.0);
            for inst in &mut self.insts {
                let (g0, g1) = (inst.igain, inst.gain_target());
                let ib = &mut inst.buf[..n];
                ib.fill(0.0);
                match &mut inst.body {
                    Body::Synth(s) => s.render_block(ib),
                    Body::Drums(d) => d.render_chunk(ib),
                }
                if g0 != 1.0 || g1 != 1.0 {
                    let step = (g1 - g0) / n as f64;
                    for (j, x) in ib.iter_mut().enumerate() {
                        *x *= (j + 1) as f64 * step + g0;
                    }
                }
                inst.igain = g1;
                for (o, x) in b.iter_mut().zip(ib.iter()) {
                    *o += x;
                }
            }
            let sm = &mut self.master_sm;
            if sm.value != sm.target {
                self.master_p = SYNTH_PARAMS[pid::MASTER_VOLUME].from_norm(sm.advance(n as i64));
            }
            let (g0, g1) = (self.master, self.master_p);
            let step = (g1 - g0) / n as f64;
            for (j, (o, x)) in block.iter_mut().zip(b.iter()).enumerate() {
                *o = (x * ((j + 1) as f64 * step + g0)).tanh();
            }
            self.master = g1;
        }
        for inst in &mut self.insts {
            inst.advance(chunk as i64);
        }
    }
}
