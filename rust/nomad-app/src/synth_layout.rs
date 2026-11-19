//! Synth panel layout, as `LAYOUT`/`HIDDEN`/`widget_kind`/`_labels`/`_short` of
//! `nomad/gui/synth_panel.py` (verified against it): sections in order, which
//! parameters are selectors, their button labels and the knobs' short names.

use nomad_core::params::{SYNTH_PARAMS, pid};

pub struct Section {
    pub key: &'static str,
    pub title: &'static str,
    /// Parameter ids, in display order (`osc1_sync` hidden; VOICE without `master_volume`).
    pub params: &'static [usize],
}

pub static SECTIONS: &[Section] = &[
    Section {
        key: "osc1",
        title: "OSC1",
        params: &[
            pid::OSC1_WAVE,
            pid::OSC1_MODE,
            pid::OSC1_COARSE,
            pid::OSC1_HZ,
            pid::OSC1_FINE,
            pid::OSC1_PW,
        ],
    },
    Section {
        key: "osc2",
        title: "OSC2",
        params: &[
            pid::OSC2_WAVE,
            pid::OSC2_MODE,
            pid::OSC2_SYNC,
            pid::OSC2_COARSE,
            pid::OSC2_HZ,
            pid::OSC2_FINE,
            pid::OSC2_PW,
        ],
    },
    Section {
        key: "mixer",
        title: "MIXER",
        params: &[pid::OSC1_LEVEL, pid::OSC2_LEVEL],
    },
    Section {
        key: "filter",
        title: "FILTER",
        params: &[pid::FILT_CUTOFF, pid::FILT_RESO, pid::FILT_ENV_AMT],
    },
    Section {
        key: "amp_env",
        title: "AMP ENV",
        params: &[
            pid::AMP_ATTACK,
            pid::AMP_DECAY,
            pid::AMP_SUSTAIN,
            pid::AMP_RELEASE,
        ],
    },
    Section {
        key: "filt_env",
        title: "FILT ENV",
        params: &[
            pid::FILT_ATTACK,
            pid::FILT_DECAY,
            pid::FILT_SUSTAIN,
            pid::FILT_RELEASE,
        ],
    },
    Section {
        key: "lfo1",
        title: "LFO1",
        params: &[
            pid::LFO1_WAVE,
            pid::LFO1_RATE,
            pid::LFO1_DEPTH,
            pid::LFO1_PHASE,
        ],
    },
    Section {
        key: "lfo2",
        title: "LFO2",
        params: &[
            pid::LFO2_WAVE,
            pid::LFO2_RATE,
            pid::LFO2_DEPTH,
            pid::LFO2_PHASE,
        ],
    },
    Section {
        key: "matrix",
        title: "MATRIX",
        params: &[
            pid::MOD0_SRC,
            pid::MOD0_DST,
            pid::MOD0_AMT,
            pid::MOD1_SRC,
            pid::MOD1_DST,
            pid::MOD1_AMT,
            pid::MOD2_SRC,
            pid::MOD2_DST,
            pid::MOD2_AMT,
            pid::MOD3_SRC,
            pid::MOD3_DST,
            pid::MOD3_AMT,
            pid::MOD4_SRC,
            pid::MOD4_DST,
            pid::MOD4_AMT,
            pid::MOD5_SRC,
            pid::MOD5_DST,
            pid::MOD5_AMT,
            pid::MOD6_SRC,
            pid::MOD6_DST,
            pid::MOD6_AMT,
            pid::MOD7_SRC,
            pid::MOD7_DST,
            pid::MOD7_AMT,
        ],
    },
    Section {
        key: "global",
        title: "VOICE",
        params: &[pid::POLYPHONY, pid::BEND_RANGE, pid::GLIDE],
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidgetKind {
    Knob,
    Selector,
}

const SEL_SUFFIX: [&str; 5] = ["_wave", "_mode", "_sync", "_src", "_dst"];

fn name(p: usize) -> &'static str {
    SYNTH_PARAMS.get(p).map_or("", |s| s.name)
}

/// Selector for discrete parameters (never a knob), knob otherwise.
pub fn widget_kind(p: usize) -> WidgetKind {
    let n = name(p);
    if n == "polyphony" || SEL_SUFFIX.iter().any(|s| n.ends_with(s)) {
        WidgetKind::Selector
    } else {
        WidgetKind::Knob
    }
}

/// Button labels of a selector (empty for knobs).
pub fn selector_labels(p: usize) -> &'static [&'static str] {
    let n = name(p);
    if n == "polyphony" {
        &["1", "2", "3", "4", "5", "6", "7", "8"]
    } else if n.ends_with("_wave") {
        if n.starts_with("lfo") {
            &["SIN", "TRI", "SAW", "SQR", "S&H"]
        } else {
            &["SIN", "TRI", "SAW", "SQR"]
        }
    } else if n.ends_with("_mode") {
        &["KEY", "FREE"]
    } else if n.ends_with("_sync") {
        &["OFF", "SYNC"]
    } else if n.ends_with("_src") {
        &["-", "LFO1", "LFO2", "MW", "VEL", "ENV2"]
    } else if n.ends_with("_dst") {
        &["-", "PIT", "P1", "P2", "AMP", "CUT", "PW1", "PW2"]
    } else {
        &[]
    }
}

/// Value of button index 0 (polyphony starts at 1 voice).
pub fn selector_offset(p: usize) -> i64 {
    i64::from(p == pid::POLYPHONY)
}

/// Knob label (`_short`): `volume`, `bend`, `osc1`/`osc2` for levels, `amt N`
/// for the matrix (also for src/dst, as in Python), else the name without its
/// section prefix.
pub fn short_label(p: usize) -> &'static str {
    let n = name(p);
    match n {
        "master_volume" => "volume",
        "bend_range" => "bend",
        _ if n.ends_with("_level") => &n[..4],
        _ if n.starts_with("mod") => {
            const AMT: [&str; 8] = [
                "amt 1", "amt 2", "amt 3", "amt 4", "amt 5", "amt 6", "amt 7", "amt 8",
            ];
            n.as_bytes()
                .get(3)
                .map_or("", |d| AMT[usize::from(d - b'0').min(7)])
        }
        _ => match n.split_once('_') {
            Some((pre, rest)) if ["osc1", "osc2", "lfo1", "lfo2", "amp", "filt"].contains(&pre) => {
                rest
            }
            _ => n,
        },
    }
}
