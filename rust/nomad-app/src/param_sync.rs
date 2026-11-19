//! Reconciles "what the user just moved" with "what the audio thread published".
//!
//! While the user drags a knob over several frames the mirror lags a little
//! (it is published at the end of the next callback): during `HOLD` after the
//! last user change the local value wins, so no increment is lost. Outside that
//! window a different, finite mirror value (MIDI, a preset, another control)
//! replaces the local one.

use std::time::{Duration, Instant};

/// How long the user's own value wins over the mirror.
pub const HOLD: Duration = Duration::from_millis(150);
/// Relative difference under which the mirror counts as equal.
pub const EPS: f64 = 1e-9;

#[derive(Clone, Debug)]
pub struct ParamSync {
    value: f64,
    last_user: Option<Instant>,
    dirty: bool,
}

impl ParamSync {
    pub fn new(initial: f64) -> ParamSync {
        ParamSync {
            value: initial,
            last_user: None,
            dirty: false,
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    /// The user moved the control: this value wins for `HOLD` and must be sent.
    pub fn user_set(&mut self, v: f64, now: Instant) {
        self.value = v;
        self.last_user = Some(now);
        self.dirty = true;
    }

    /// The mirror's value this frame.
    pub fn observe(&mut self, mirror: f64, now: Instant) {
        if !mirror.is_finite() {
            return;
        }
        let holding = self
            .last_user
            .is_some_and(|t| now.duration_since(t) <= HOLD);
        let differs = (mirror - self.value).abs() > EPS * mirror.abs().max(self.value.abs());
        if !holding && differs {
            self.value = mirror;
        }
    }

    pub fn needs_send(&self) -> bool {
        self.dirty
    }

    pub fn mark_sent(&mut self) {
        self.dirty = false;
    }
}

/// One `ParamSync` per synth parameter.
#[derive(Clone, Debug)]
pub struct SynthParams {
    params: Vec<ParamSync>,
}

impl SynthParams {
    pub fn new(initial: &[f64]) -> SynthParams {
        SynthParams {
            params: initial.iter().map(|&v| ParamSync::new(v)).collect(),
        }
    }

    /// The value to show for `pid` this frame, after looking at the mirror.
    pub fn ui_value(&mut self, pid: usize, mirror: f64, now: Instant) -> f64 {
        match self.params.get_mut(pid) {
            Some(p) => {
                p.observe(mirror, now);
                p.value()
            }
            None => mirror,
        }
    }

    pub fn user_set(&mut self, pid: usize, v: f64, now: Instant) {
        if let Some(p) = self.params.get_mut(pid) {
            p.user_set(v, now);
        }
    }

    pub fn get_mut(&mut self, pid: usize) -> Option<&mut ParamSync> {
        self.params.get_mut(pid)
    }
}
