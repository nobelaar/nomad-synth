use nomad_core::mods::Mods;
use nomad_core::params::{ParamValues, SYNTH_PARAMS, default_values, pid};

const PAIRS: [(usize, &str); 63] = [
    (pid::POLYPHONY, "polyphony"),
    (pid::GLIDE, "glide"),
    (pid::MASTER_VOLUME, "master_volume"),
    (pid::OSC1_WAVE, "osc1_wave"),
    (pid::OSC1_MODE, "osc1_mode"),
    (pid::OSC1_COARSE, "osc1_coarse"),
    (pid::OSC1_HZ, "osc1_hz"),
    (pid::OSC1_FINE, "osc1_fine"),
    (pid::OSC1_PW, "osc1_pw"),
    (pid::OSC1_LEVEL, "osc1_level"),
    (pid::OSC1_SYNC, "osc1_sync"),
    (pid::OSC2_WAVE, "osc2_wave"),
    (pid::OSC2_MODE, "osc2_mode"),
    (pid::OSC2_COARSE, "osc2_coarse"),
    (pid::OSC2_HZ, "osc2_hz"),
    (pid::OSC2_FINE, "osc2_fine"),
    (pid::OSC2_PW, "osc2_pw"),
    (pid::OSC2_LEVEL, "osc2_level"),
    (pid::OSC2_SYNC, "osc2_sync"),
    (pid::FILT_CUTOFF, "filt_cutoff"),
    (pid::FILT_RESO, "filt_reso"),
    (pid::FILT_ENV_AMT, "filt_env_amt"),
    (pid::AMP_ATTACK, "amp_attack"),
    (pid::AMP_DECAY, "amp_decay"),
    (pid::AMP_SUSTAIN, "amp_sustain"),
    (pid::AMP_RELEASE, "amp_release"),
    (pid::FILT_ATTACK, "filt_attack"),
    (pid::FILT_DECAY, "filt_decay"),
    (pid::FILT_SUSTAIN, "filt_sustain"),
    (pid::FILT_RELEASE, "filt_release"),
    (pid::LFO1_RATE, "lfo1_rate"),
    (pid::LFO1_WAVE, "lfo1_wave"),
    (pid::LFO1_DEPTH, "lfo1_depth"),
    (pid::LFO1_PHASE, "lfo1_phase"),
    (pid::LFO2_RATE, "lfo2_rate"),
    (pid::LFO2_WAVE, "lfo2_wave"),
    (pid::LFO2_DEPTH, "lfo2_depth"),
    (pid::LFO2_PHASE, "lfo2_phase"),
    (pid::MOD0_SRC, "mod0_src"),
    (pid::MOD0_DST, "mod0_dst"),
    (pid::MOD0_AMT, "mod0_amt"),
    (pid::MOD1_SRC, "mod1_src"),
    (pid::MOD1_DST, "mod1_dst"),
    (pid::MOD1_AMT, "mod1_amt"),
    (pid::MOD2_SRC, "mod2_src"),
    (pid::MOD2_DST, "mod2_dst"),
    (pid::MOD2_AMT, "mod2_amt"),
    (pid::MOD3_SRC, "mod3_src"),
    (pid::MOD3_DST, "mod3_dst"),
    (pid::MOD3_AMT, "mod3_amt"),
    (pid::MOD4_SRC, "mod4_src"),
    (pid::MOD4_DST, "mod4_dst"),
    (pid::MOD4_AMT, "mod4_amt"),
    (pid::MOD5_SRC, "mod5_src"),
    (pid::MOD5_DST, "mod5_dst"),
    (pid::MOD5_AMT, "mod5_amt"),
    (pid::MOD6_SRC, "mod6_src"),
    (pid::MOD6_DST, "mod6_dst"),
    (pid::MOD6_AMT, "mod6_amt"),
    (pid::MOD7_SRC, "mod7_src"),
    (pid::MOD7_DST, "mod7_dst"),
    (pid::MOD7_AMT, "mod7_amt"),
    (pid::BEND_RANGE, "bend_range"),
];

#[test]
fn every_pid_matches_its_name() {
    for (i, (id, name)) in PAIRS.iter().enumerate() {
        assert_eq!(*id, i, "{name}");
        assert_eq!(SYNTH_PARAMS[*id].name, *name);
    }
}

#[test]
fn values_array_has_63_entries() {
    let v: ParamValues = default_values();
    assert_eq!(v.len(), SYNTH_PARAMS.len());
    assert_eq!(pid::COUNT, 63);
}

#[test]
fn default_values_match_registry() {
    let v = default_values();
    for (x, s) in v.iter().zip(&SYNTH_PARAMS) {
        assert_eq!(*x, s.default, "{}", s.name);
    }
}

#[test]
fn mods_default_is_zero() {
    let m = Mods::default();
    assert_eq!(
        m,
        Mods {
            pitch: 0.0,
            pitch1: 0.0,
            pitch2: 0.0,
            amp: 0.0,
            cutoff: 0.0,
            pw1: 0.0,
            pw2: 0.0,
            bend: 0.0
        }
    );
}
