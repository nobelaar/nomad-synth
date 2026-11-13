import math

import pytest

from nomad.engine.lfo import LFO


def _run(lfo, n, dt=0.001):
    return [lfo.advance(dt) for _ in range(n)]


def test_rate_is_correct():
    lfo = LFO(48000); lfo.set(2.0, 0, 1.0, 0.0)   # 2 Hz, sine, 1 s
    vals = _run(lfo, 1000)
    crossings = sum(1 for a, b in zip(vals, vals[1:]) if a < 0 <= b)
    assert 1 <= crossings <= 3


def test_depth_scales_output():
    lfo = LFO(48000); lfo.set(1.0, 0, 0.0, 0.0)
    assert all(lfo.advance(0.001) == 0.0 for _ in range(100))


def test_sample_and_hold_is_reproducible():
    l1 = LFO(48000, seed=7); l2 = LFO(48000, seed=7)
    l1.set(5.0, 4, 1.0, 0.0); l2.set(5.0, 4, 1.0, 0.0)
    assert _run(l1, 500) == _run(l2, 500)


def test_phase_offset_shifts_start():
    lfo = LFO(48000); lfo.set(1.0, 0, 1.0, 0.25)   # sine arranca en el pico
    assert abs(lfo.advance(0.0001) - 1.0) < 0.01


@pytest.mark.parametrize("wave", range(5))
def test_all_waves_bounded_and_finite(wave):
    lfo = LFO(48000, seed=3); lfo.set(7.3, wave, 0.6, 0.1)
    vals = _run(lfo, 20000, 0.0053)
    assert all(math.isfinite(v) and abs(v) <= 0.6 + 1e-12 for v in vals)


def _shape(wave, phases):
    out = []
    for p in phases:
        lfo = LFO(48000); lfo.set(1.0, wave, 1.0, 0.0)
        out.append(lfo.advance(p))
    return out


def test_saw_rises():
    v = _shape(2, [0.25, 0.5, 0.75])
    assert v == pytest.approx([-0.5, 0.0, 0.5])


def test_triangle_shape():
    v = _shape(1, [0.25, 0.5, 0.75])
    assert v == pytest.approx([0.0, 1.0, 0.0]) or v == pytest.approx([0.0, -1.0, 0.0])
    assert abs(_shape(1, [0.5])[0]) == pytest.approx(1.0)


def test_square_halves():
    v = _shape(3, [0.25, 0.75])
    assert v == [1.0, -1.0]


def test_sample_hold_holds_within_cycle_and_changes_at_boundary():
    lfo = LFO(48000, seed=1); lfo.set(1.0, 4, 1.0, 0.0)
    first = _run(lfo, 9, 0.1)            # phase 0.1..0.9
    assert len(set(first)) == 1
    nxt = _run(lfo, 2, 0.1)              # crosses 1.0
    assert nxt[-1] != first[0]
    assert len(set(_run(lfo, 5, 0.1))) <= 2


def test_sample_hold_different_seeds_differ():
    a = LFO(48000, seed=1); b = LFO(48000, seed=2)
    a.set(1.0, 4, 1.0, 0.0); b.set(1.0, 4, 1.0, 0.0)
    assert a.advance(0.1) != b.advance(0.1)


def test_phase_offset_wraps():
    a = LFO(48000); a.set(1.0, 2, 1.0, 1.25)
    b = LFO(48000); b.set(1.0, 2, 1.0, 0.25)
    c = LFO(48000); c.set(1.0, 2, 1.0, -0.75)
    assert a.advance(0.1) == pytest.approx(b.advance(0.1)) == pytest.approx(c.advance(0.1))


@pytest.mark.parametrize("bad", [float("nan"), float("inf"), -float("inf"), -1.0])
def test_bad_params_are_safe(bad):
    lfo = LFO(48000); lfo.set(bad, 0, bad, bad)
    assert all(math.isfinite(v) for v in _run(lfo, 50))
    lfo.set(1.0, 0, 1.0, 0.0)
    assert math.isfinite(lfo.advance(bad))


@pytest.mark.parametrize("wave", [-1, 5, 99])
def test_wave_out_of_range_raises(wave):
    with pytest.raises(ValueError):
        LFO(48000).set(1.0, wave, 1.0, 0.0)


def test_large_advance_keeps_phase_in_range():
    lfo = LFO(48000); lfo.set(3.0, 0, 1.0, 0.0)
    lfo.advance(12345.678)
    assert 0.0 <= lfo.phase < 1.0
