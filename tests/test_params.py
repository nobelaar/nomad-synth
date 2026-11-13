import pytest
from nomad.engine.params import REGISTRY, ParamSpec, clamp, to_norm, from_norm


def test_every_param_has_sane_default():
    for name, p in REGISTRY.items():
        assert p.lo <= p.default <= p.hi, name


def test_clamp_respects_range():
    assert clamp("filt_cutoff", 1e9) == REGISTRY["filt_cutoff"].hi
    assert clamp("filt_cutoff", -5) == REGISTRY["filt_cutoff"].lo


def test_exp_curve_roundtrip():
    v = 1200.0
    assert from_norm("filt_cutoff", to_norm("filt_cutoff", v)) == pytest.approx(v, rel=1e-9)


def test_unknown_param_raises():
    with pytest.raises(KeyError):
        clamp("no_existe", 1.0)


def test_smoother_approaches_target_without_overshoot():
    from nomad.engine.params import Smoother

    s = Smoother(0.0, 48000.0, 15.0)
    s.set(1.0)
    vals = [s.advance(16) for _ in range(100)]
    assert vals[-1] == pytest.approx(1.0, abs=1e-3)
    assert all(0.0 <= v <= 1.0 for v in vals)
