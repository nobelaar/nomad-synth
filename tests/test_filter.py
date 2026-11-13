# tests/test_filter.py
import numpy as np
import pytest
from scipy.signal import freqz
from nomad.engine.filter import Ladder4Pole, ladder_coeffs

SR = 48000.0

def peak_db(b, a, lo=200, hi=4000):
    w, h = freqz(b, a, worN=8192, fs=SR)
    m = (w > lo) & (w < hi)
    return 20*np.log10(np.abs(h[m]).max() + 1e-12)

def test_resonance_raises_the_peak():
    b0, a0 = ladder_coeffs(1000.0, 0.0, SR)
    b1, a1 = ladder_coeffs(1000.0, 3.9, SR)
    assert peak_db(b1, a1) - peak_db(b0, a0) > 15

def test_stable_below_threshold():
    b, a = ladder_coeffs(1000.0, 3.9, SR)
    assert np.abs(np.roots(a)).max() < 1.0

def test_self_oscillation_is_bounded():
    f = Ladder4Pole(SR); f.set(440.0, 1.0)
    x = np.zeros(48000); rng = np.random.default_rng(0)
    x[:200] = rng.standard_normal(200)
    for i in range(0, len(x), 16):
        f.process(x[i:i+16])
    assert np.isfinite(x).all()
    assert np.abs(x[-24000:]).max() < 10.0

def test_no_nan_at_extremes():
    for fc in (20.0, 18000.0):
        for res in (0.0, 0.5, 1.0):
            f = Ladder4Pole(SR); f.set(fc, res)
            buf = np.zeros(256 // 4); buf[:] = 0.5
            f.process(buf[:16]); f.process(buf[16:])
            assert np.isfinite(buf).all()

def test_grain_limit_is_64():
    f = Ladder4Pole(SR); f.set(1000.0, 0.5)  # k=2.15 <= 3.2
    buf = np.ones(64); f.process(buf)
    assert np.isfinite(buf).all()
    with pytest.raises(ValueError):
        f.process(np.ones(65))

def test_dc_gain_and_corner_at_k0():
    b, a = ladder_coeffs(1000.0, 0.0, SR)
    w, h = freqz(b, a, worN=[1.0, 1000.0], fs=SR)
    assert abs(abs(h[0]) - 1.0) < 1e-3
    # 4 cascaded poles: -3 dB point of each is at fc, so total is -12 dB there
    assert -14 < 20*np.log10(abs(h[1])) < -10

def test_bad_params_and_reset():
    f = Ladder4Pole(SR)
    for fc, r in ((float("nan"), 0.5), (float("inf"), float("nan")), (-5.0, 2.0)):
        f.set(fc, r)
        buf = np.full(16, 0.3); f.process(buf)
        assert np.isfinite(buf).all()
    f.set(1000.0, 0.0)
    f.process(np.ones(16)); f.reset()
    z = np.zeros(16); f.process(z)
    assert not z.any()
