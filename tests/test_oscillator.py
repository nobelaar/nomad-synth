import numpy as np
import pytest
from nomad.engine.oscillator import Oscillator

SR = 48000.0


def render(wave, freq, n=64, **kw):
    o = Oscillator(SR); o.wave = wave
    out = np.zeros(n); o.render_into(out, freq, **kw)
    return out


def in_chunks(wave, freq, n, grain=16, **kw):
    o = Oscillator(SR); o.wave = wave
    out = np.zeros(n)
    for i in range(0, n, grain):
        o.render_into(out[i:i+grain], freq, **kw)
    return out


def test_sine_matches_reference():
    out = render(0, 440.0, 64)
    ref = np.sin(2*np.pi*440.0*np.arange(64)/SR)
    assert np.allclose(out, ref, atol=1e-6)


def test_phase_continues_across_calls():
    o = Oscillator(SR); o.wave = 2
    a = np.zeros(64); b = np.zeros(64)
    o.render_into(a, 440.0); o.render_into(b, 440.0)
    o2 = Oscillator(SR); o2.wave = 2
    whole = np.zeros(128); o2.render_into(whole, 440.0)
    assert np.allclose(np.concatenate([a, b]), whole, atol=1e-9)


def test_polyblep_reduces_aliasing_vs_naive():
    # 5236 Hz at 48 kHz: aliases must fall below -30 dB with BLEP.
    out = render(2, 5236.0, 4096)
    spec = np.abs(np.fft.rfft(out * np.hanning(len(out))))
    f = np.fft.rfftfreq(len(out), 1/SR)
    alias_band = (f > 8000) & (f < 16000)
    # A 5236 Hz saw has real harmonics at 10472 and 15708 Hz inside that band;
    # without excluding them the band maximum is the 2nd harmonic (-7 dB) and
    # no implementation could pass. Drop the window's main lobe (+-3 bins)
    # around every harmonic.
    for n in range(1, 10):
        alias_band &= np.abs(f - n * 5236.0) > 60.0
    assert 20*np.log10(spec[alias_band].max() / spec.max()) < -30


def test_square_duty_is_half_at_pw_50():
    out = render(3, 200.0, 4800, pw=0.5)
    assert abs((out > 0).mean() - 0.5) < 0.02


def test_render_into_adds_to_the_buffer():
    base = render(2, 440.0, 64)
    o = Oscillator(SR); o.wave = 2
    buf = np.full(64, 0.25)
    o.render_into(buf, 440.0)
    assert np.allclose(buf, base + 0.25, atol=1e-12)


def test_chunked_rendering_matches_one_block_for_every_wave():
    for wave in (0, 1, 2, 3):
        whole = render(wave, 440.0, 4800)
        assert np.allclose(in_chunks(wave, 440.0, 4800), whole, atol=1e-9), wave


def test_all_waves_stay_bounded_and_centred_over_many_chunks():
    for wave in (0, 1, 2, 3):
        out = in_chunks(wave, 55.0, 48000)
        assert np.isfinite(out).all(), wave
        assert np.abs(out).max() < 1.05, wave
        assert abs(out.mean()) < 0.01, wave


def test_zero_frequency_holds_phase_and_stays_finite():
    for wave in (0, 1, 2, 3):
        o = Oscillator(SR); o.wave = wave
        out = np.zeros(64)
        assert o.render_into(out, 0.0) == 0.0
        assert np.isfinite(out).all(), wave


def test_triangle_is_centred_and_symmetric_at_high_pitch():
    # Shape, not just bounds: a left-Riemann phase integral shifts the triangle
    # by half a sample, which at 2 kHz means -2*inc of DC (-0.083) and a
    # positive peak short by 4*inc (0.833). The plan asks the triangle to be
    # normalized by its theoretical maximum, so it must stay centred and
    # symmetric up to high notes.
    out = render(1, 2000.0, 48000)
    assert abs(out.mean()) < 1e-3
    assert abs(abs(out.max()) - abs(out.min())) < 0.01
    assert abs(np.abs(out).max() - 1.0) < 0.01


def test_non_finite_pw_does_not_poison_the_triangle():
    o = Oscillator(SR); o.wave = 1
    poisoned = np.zeros(512)
    o.render_into(poisoned, 200.0, pw=float("nan"))
    assert np.isfinite(poisoned).all()
    # Same phase, healthy pw: the bad call must leave the oscillator exactly
    # where it belongs, so both continuations are identical.
    ref = Oscillator(SR); ref.wave = 1
    ref.render_into(np.zeros(512), 200.0)
    cont_poisoned = np.zeros(512); o.render_into(cont_poisoned, 200.0)
    cont_ref = np.zeros(512); ref.render_into(cont_ref, 200.0)
    assert np.allclose(cont_poisoned, cont_ref, atol=1e-12)


def test_invalid_wave_raises():
    o = Oscillator(SR)
    for wave in (4, -1):
        o.wave = wave
        with pytest.raises(ValueError):
            o.render_into(np.zeros(16), 440.0)


def test_render_into_reuses_its_block_buffers(monkeypatch):
    # The engine requires render() not to build new buffers (Task 10), and a
    # note-on creates oscillators inside the audio callback, so this must hold
    # from the very first call, with no warm-up.
    o = Oscillator(SR); o.wave = 1
    buf = np.zeros(16)
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1
            return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(20):
        o.render_into(buf, 440.0)
    assert calls["n"] == 0
