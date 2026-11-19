//! One-pole exponential parameter smoother, ported from `nomad/engine/params.py`.

#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub target: f64,
    pub value: f64,
    pub sample_rate: f64,
    pub time_ms: f64,
}

impl Smoother {
    pub fn new(target: f64, sample_rate: f64, time_ms: f64) -> Smoother {
        Smoother {
            target,
            value: target,
            sample_rate,
            time_ms,
        }
    }

    /// Move the target; `value` follows on the next advances.
    pub fn set(&mut self, target: f64) {
        self.target = target;
    }

    /// Advance `frames` samples towards the target and return the value.
    pub fn advance(&mut self, frames: i64) -> f64 {
        if frames <= 0 {
            return self.value;
        }
        // Minimum tau keeps time_ms = 0 from dividing by zero (as in Python).
        let tau = (self.time_ms / 1000.0 * self.sample_rate).max(1e-9);
        self.value += (1.0 - (-(frames as f64) / tau).exp()) * (self.target - self.value);
        self.value
    }
}
