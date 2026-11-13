"""Numba kernels vs. their scipy / pure-Python references (R9)."""
import numpy as np
import pytest
from scipy.signal import lfilter

from nomad.engine import _kernels
from nomad.engine.envelope import Envelope
from nomad.engine.filter import K_MAX, Ladder4Pole, ladder_coeffs

SR = 48000.0
pytestmark = pytest.mark.skipif(not _kernels.HAVE_NUMBA, reason="numba not installed")


@pytest.mark.parametrize("fc,res", [(100.0, 0.0), (1000.0, 0.5), (440.0, 0.8), (440.0, 1.0), (9000.0, 1.0)])
def test_ladder_kernel_matches_lfilter(fc, res, monkeypatch):
    x = np.random.default_rng(1).standard_normal(4096) * 0.5
    fast = Ladder4Pole(SR); fast.set(fc, res)
    ref = Ladder4Pole(SR); ref.set(fc, res)
    grain = 16 if res * K_MAX > 3.2 else 64
    y_fast = x.copy()
    y_ref = x.copy()
    for s in range(0, len(x), grain):
        fast.process(y_fast[s:s + grain])
        with monkeypatch.context() as m:
            m.setattr(_kernels, "HAVE_NUMBA", False)
            ref.process(y_ref[s:s + grain])
    assert np.allclose(y_fast, y_ref, atol=1e-9, rtol=0)
    assert np.allclose(fast._zi, ref._zi, atol=1e-9, rtol=0)


def test_coeff_kernel_matches_closed_form():
    b, a = np.empty(5), np.empty(5)
    for fc, k in [(50.0, 0.0), (1000.0, 2.0), (12000.0, K_MAX)]:
        _kernels.ladder_coeffs_into(b, a, fc, k, SR)
        rb, ra = ladder_coeffs(fc, k, SR)
        assert np.allclose(b, rb, rtol=1e-12, atol=1e-15) and np.allclose(a, ra, rtol=1e-12, atol=1e-15)


def test_filter_fallback_path_works(monkeypatch):
    monkeypatch.setattr(_kernels, "HAVE_NUMBA", False)
    f = Ladder4Pole(SR); f.set(800.0, 1.0)
    x = np.random.default_rng(2).standard_normal(2048)
    for s in range(0, len(x), 16):
        f.process(x[s:s + 16])
    assert np.isfinite(x).all() and np.abs(x).max() < 50


def _run_env(env_run, monkeypatch):
    monkeypatch.setattr(_kernels, "env_run", env_run)
    e = Envelope(SR); e.set_params(0.005, 0.02, 0.4, 0.03)
    chunks = []
    def blk(n=256):
        o = np.empty(n); e.process(o); chunks.append(o.copy())
    e.note_on(); blk(); blk()
    e.note_off(); blk(); blk(); blk()  # release and idle
    e.note_on(); blk(64); e.note_on(); blk(); e.note_off(); blk(); blk()  # retrigger mid-attack, release
    return np.concatenate(chunks)


def test_envelope_kernel_matches_pure_python(monkeypatch):
    jitted = _run_env(_kernels.env_run, monkeypatch)
    plain = _run_env(_kernels.env_run.py_func, monkeypatch)
    assert np.allclose(jitted, plain, atol=1e-12, rtol=0)
