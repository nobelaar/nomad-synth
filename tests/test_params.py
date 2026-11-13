import math

import pytest
from nomad.engine.params import REGISTRY, ParamSpec, Smoother, clamp, from_norm, to_norm

EXPECTED_NAMES = {
    "polyphony",
    "master_volume",
    "osc1_wave",
    "osc1_mode",
    "osc1_coarse",
    "osc1_hz",
    "osc1_fine",
    "osc1_pw",
    "osc1_level",
    "osc1_sync",
    "osc2_wave",
    "osc2_mode",
    "osc2_coarse",
    "osc2_hz",
    "osc2_fine",
    "osc2_pw",
    "osc2_level",
    "osc2_sync",
    "filt_cutoff",
    "filt_reso",
    "filt_env_amt",
    "amp_attack",
    "amp_decay",
    "amp_sustain",
    "amp_release",
    "filt_attack",
    "filt_decay",
    "filt_sustain",
    "filt_release",
    "lfo1_rate",
    "lfo1_wave",
    "lfo1_depth",
    "lfo1_phase",
    "lfo2_rate",
    "lfo2_wave",
    "lfo2_depth",
    "lfo2_phase",
    "mod0_src",
    "mod0_dst",
    "mod0_amt",
    "mod1_src",
    "mod1_dst",
    "mod1_amt",
    "mod2_src",
    "mod2_dst",
    "mod2_amt",
    "mod3_src",
    "mod3_dst",
    "mod3_amt",
    "mod4_src",
    "mod4_dst",
    "mod4_amt",
    "mod5_src",
    "mod5_dst",
    "mod5_amt",
    "mod6_src",
    "mod6_dst",
    "mod6_amt",
    "mod7_src",
    "mod7_dst",
    "mod7_amt",
    "bend_range",
}


def test_registry_has_exactly_the_expected_names():
    assert len(REGISTRY) == 62
    assert set(REGISTRY) == EXPECTED_NAMES
    for name, spec in REGISTRY.items():
        assert spec.name == name


@pytest.mark.parametrize(
    ("name", "default", "lo", "hi", "curve", "unit"),
    [
        ("filt_cutoff", 1200.0, 20.0, 18000.0, "exp", "Hz"),
        ("lfo1_rate", 1.0, 0.02, 40.0, "exp", "Hz"),
        ("amp_attack", 5.0, 1.0, 5000.0, "exp", "ms"),
        ("amp_sustain", 0.8, 0.0, 1.0, "lin", ""),
        ("mod0_amt", 0.0, -1.0, 1.0, "lin", ""),
        ("polyphony", 8.0, 1.0, 8.0, "lin", "voices"),
        ("osc1_pw", 0.5, 0.05, 0.95, "lin", ""),
        ("osc1_coarse", 0.0, -24.0, 24.0, "lin", "st"),
    ],
)
def test_registry_pins_representative_specs(name, default, lo, hi, curve, unit):
    spec = REGISTRY[name]
    assert (spec.default, spec.lo, spec.hi) == (default, lo, hi)
    assert spec.curve == curve and spec.unit == unit


def test_every_param_has_sane_default():
    for name, p in REGISTRY.items():
        assert p.lo <= p.default <= p.hi, name


def test_clamp_respects_range():
    assert clamp("filt_cutoff", 1e9) == REGISTRY["filt_cutoff"].hi
    assert clamp("filt_cutoff", -5) == REGISTRY["filt_cutoff"].lo


def test_clamp_replaces_nan_with_default():
    assert clamp("filt_reso", float("nan")) == REGISTRY["filt_reso"].default


def test_exp_curve_roundtrip():
    v = 1200.0
    assert from_norm("filt_cutoff", to_norm("filt_cutoff", v)) == pytest.approx(v, rel=1e-9)


def test_unknown_param_raises():
    with pytest.raises(KeyError):
        clamp("no_existe", 1.0)


def test_smoother_approaches_target_without_overshoot():
    s = Smoother(0.0, 48000.0, 15.0)
    s.set(1.0)
    vals = [s.advance(16) for _ in range(400)]
    assert vals[-1] == pytest.approx(1.0, abs=1e-3)
    assert all(0.0 <= v <= 1.0 for v in vals)


def test_smoother_uses_time_ms_as_time_constant():
    s = Smoother(0.0, 48000.0, 15.0)
    s.set(1.0)
    assert s.advance(16) == pytest.approx(1.0 - math.exp(-16.0 / (0.015 * 48000.0)))


def test_smoother_tracks_mutable_time_ms():
    s = Smoother(0.0, 48000.0, 15.0)
    s.time_ms = 30.0
    s.set(1.0)
    expected = 1.0 - math.exp(-16.0 / (0.030 * 48000.0))
    assert s.advance(16) == pytest.approx(expected)
