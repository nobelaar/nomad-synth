//! TAP tempo, as `InstanceStrip.tap` in `nomad/gui/strip.py`: keep up to the
//! last five taps (four intervals), forget them after a pause of 2 s or more.

use std::time::Instant;

const KEEP: usize = 4;
const PAUSE_S: f64 = 2.0;

#[derive(Default)]
pub struct TapTempo {
    taps: Vec<Instant>,
}

impl TapTempo {
    pub fn new() -> TapTempo {
        TapTempo::default()
    }

    /// Register a tap; from the second one on, the tempo (rounded half to even
    /// like Python's `round`, clamped to 20..300).
    pub fn tap(&mut self, now: Instant) -> Option<f64> {
        let recent = self
            .taps
            .last()
            .is_some_and(|&t| now.duration_since(t).as_secs_f64() < PAUSE_S);
        if !recent {
            self.taps.clear();
        }
        if self.taps.len() > KEEP {
            self.taps.drain(..self.taps.len() - KEEP);
        }
        self.taps.push(now);
        let (first, last) = (self.taps.first()?, self.taps.last()?);
        if self.taps.len() < 2 {
            return None;
        }
        let span = last.duration_since(*first).as_secs_f64();
        let bpm = (60.0 * (self.taps.len() - 1) as f64 / span).round_ties_even();
        Some(bpm.clamp(20.0, 300.0))
    }
}
