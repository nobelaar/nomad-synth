//! Per-voice modulation amounts, as `voice.py` receives them in its `mods` dict.

/// `pitch*` and `bend` are in semitones; the rest are bipolar amounts.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Mods {
    pub pitch: f64,
    pub pitch1: f64,
    pub pitch2: f64,
    pub amp: f64,
    pub cutoff: f64,
    pub pw1: f64,
    pub pw2: f64,
    pub bend: f64,
}
