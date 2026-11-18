//! The demo scene: factory presets "Pad de acordes" (synth) and "4x4" (drums)
//! from `nomad/presets.py` `FACTORY`, written out here.

use nomad_core::engine::{Engine, InstrumentId};
use nomad_core::params::SYNTH_PARAMS;

pub const SYNTH: InstrumentId = 1;
pub const DRUMS: InstrumentId = 2;
const NONE: u8 = 255;

/// Values of "Pad de acordes" that differ from the registry defaults.
const PAD: [(&str, f64); 22] = [
    ("polyphony", 6.0),
    ("osc1_fine", -9.0),
    ("osc1_level", 0.45),
    ("osc2_fine", 9.0),
    ("osc2_level", 0.45),
    ("filt_cutoff", 2200.0),
    ("filt_reso", 0.2),
    ("filt_env_amt", 0.25),
    ("amp_attack", 900.0),
    ("amp_decay", 800.0),
    ("amp_sustain", 0.85),
    ("amp_release", 2500.0),
    ("filt_attack", 1200.0),
    ("filt_decay", 1500.0),
    ("filt_sustain", 0.5),
    ("filt_release", 2000.0),
    ("lfo1_rate", 0.25),
    ("lfo1_wave", 1.0),
    ("lfo1_depth", 1.0),
    ("mod0_src", 1.0),
    ("mod0_dst", 5.0),
    ("mod0_amt", 0.15),
];
const PAD_NOTES: [u8; 4] = [45, 41, 48, 43];
const PAD_EXTRA: [[u8; 2]; 4] = [[48, 52], [45, 48], [52, 55], [47, 50]];
/// "4x4": per track (kick, snare, clap, tom, chat, ohat) the (step, level) hits.
const FOUR: [&[(usize, u8)]; 6] = [
    &[(0, 2), (4, 1), (8, 1), (12, 1)],
    &[],
    &[(4, 1), (12, 1)],
    &[],
    &[(2, 1), (6, 1), (10, 1), (14, 1)],
    &[],
];

fn pid(name: &str) -> usize {
    SYNTH_PARAMS
        .iter()
        .position(|s| s.name == name)
        .expect("known synth param")
}

/// Load the pad preset into synth `id` (sound + chord pattern, sequencer on).
pub fn load_pad(e: &mut Engine, id: InstrumentId) {
    let Some(s) = e.synth_mut(id) else { return };
    for (name, v) in PAD {
        s.set_param(pid(name), v);
    }
    let sq = &mut s.sequencer;
    sq.set_gate(1.0);
    sq.grid.division = 4.0;
    sq.grid.length = 4;
    for (k, (&n, extra)) in PAD_NOTES.iter().zip(&PAD_EXTRA).enumerate() {
        sq.set_step(k, true, Some(i64::from(n)), Some(90));
        sq.extra[k] = [extra[0], extra[1], NONE, NONE, NONE];
    }
    sq.enabled = true;
}

/// Load the "4x4" drum pattern into machine `id`.
pub fn load_four(e: &mut Engine, id: InstrumentId) {
    let Some(d) = e.drums_mut(id) else { return };
    d.clear_pattern();
    for (track, hits) in FOUR.iter().enumerate() {
        for &(step, level) in *hits {
            d.set_step(track, step, level);
        }
    }
}

/// Default engine with the pad and the 4x4 beat, both transports running.
pub fn demo_engine(sample_rate: f64) -> Engine {
    let mut e = Engine::new(sample_rate, 8);
    load_pad(&mut e, SYNTH);
    load_four(&mut e, DRUMS);
    e.start(SYNTH);
    e.start(DRUMS);
    e
}
