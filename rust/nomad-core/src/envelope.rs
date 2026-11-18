//! ADSR envelope with exponential segments, ported from `nomad/engine/envelope.py`
//! (the sample loop is `env_run` of `nomad/engine/_kernels.py`).

pub const MIN_TIME: f64 = 0.001;
const OVERSHOOT: f64 = 1.3;
const FLOOR: f64 = 1e-4;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

fn time(x: f64) -> f64 {
    if !x.is_finite() || x < MIN_TIME {
        MIN_TIME
    } else {
        x
    }
}

pub struct Envelope {
    /// Last emitted level.
    pub value: f64,
    sr: f64,
    stage: Stage,
    coef: f64,
    a: f64,
    d: f64,
    s: f64,
    r: f64,
}

impl Envelope {
    pub fn new(sample_rate: f64) -> Envelope {
        let mut e = Envelope {
            value: 0.0,
            sr: sample_rate,
            stage: Stage::Idle,
            coef: 0.0,
            a: MIN_TIME,
            d: MIN_TIME,
            s: 0.0,
            r: MIN_TIME,
        };
        e.set_params(0.01, 0.1, 0.7, 0.2);
        e
    }

    pub fn set_params(&mut self, attack_s: f64, decay_s: f64, sustain: f64, release_s: f64) {
        let (a, d, sus, r) = (self.a, self.d, self.s, self.r);
        self.a = time(attack_s);
        self.d = time(decay_s);
        self.r = time(release_s);
        self.s = if sustain.is_finite() {
            sustain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Re-enter only when the running stage's own parameter changed: callers push
        // params every control block and re-entering would restart the attack forever.
        match self.stage {
            Stage::Attack if self.a != a => self.enter(Stage::Attack),
            Stage::Decay | Stage::Sustain if self.d != d || self.s != sus => {
                self.enter(Stage::Decay)
            }
            Stage::Release if self.r != r => self.enter(Stage::Release),
            _ => {}
        }
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        match stage {
            Stage::Attack if self.value >= 1.0 => self.enter(Stage::Decay),
            Stage::Attack => {
                let n = self.a * self.sr;
                self.coef = ((OVERSHOOT - 1.0) / (OVERSHOOT - self.value)).powf(1.0 / n);
            }
            Stage::Decay => self.coef = 1e-3_f64.powf(1.0 / (self.d * self.sr)),
            Stage::Release => self.coef = FLOOR.powf(1.0 / (self.r * self.sr)),
            Stage::Idle | Stage::Sustain => {}
        }
    }

    pub fn note_on(&mut self) {
        self.enter(Stage::Attack);
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.enter(Stage::Release);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    /// Fill `out` with one envelope sample per frame.
    pub fn process(&mut self, out: &mut [f64]) {
        let dcoef = 1e-3_f64.powf(1.0 / (self.d * self.sr));
        let (mut v, mut c, mut st, s) = (self.value, self.coef, self.stage, self.s);
        for o in out.iter_mut() {
            match st {
                Stage::Attack => {
                    v = OVERSHOOT + (v - OVERSHOOT) * c;
                    if v >= 1.0 {
                        v = 1.0;
                        st = Stage::Decay;
                        c = dcoef;
                    }
                }
                Stage::Decay => {
                    v = s + (v - s) * c;
                    if (v - s).abs() < 1e-5 {
                        v = s;
                        st = Stage::Sustain;
                    }
                }
                Stage::Release => {
                    v *= c;
                    if v < FLOOR {
                        v = 0.0;
                        st = Stage::Idle;
                    }
                }
                Stage::Idle | Stage::Sustain => {}
            }
            *o = v;
        }
        (self.value, self.coef, self.stage) = (v, c, st);
    }
}
