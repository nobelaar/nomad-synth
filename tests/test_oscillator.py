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


def test_pulse_width_changes_duty():
    out = render(3, 200.0, 4800, pw=0.25)
    assert abs((out > 0).mean() - 0.25) < 0.02


def test_pw_is_clamped():
    out = render(3, 200.0, 4800, pw=0.0)
    assert abs((out > 0).mean() - 0.05) < 0.02


def test_pw_is_ignored_by_every_wave_but_square():
    # Duty cycle is a property of the square only: a triangle is symmetric by
    # definition and the sine and saw have no width to modulate. The default
    # pw must also stay bit-identical to the pre-pw-precision waveform.
    for wave in (0, 1, 2):
        narrow = render(wave, 200.0, 1024, pw=0.2)
        wide = render(wave, 200.0, 1024, pw=0.8)
        assert np.array_equal(narrow, wide), wave


def test_sync_resets_phase_at_master_wrap():
    master = np.concatenate([np.linspace(0, 0.9, 32), np.linspace(0, 0.9, 32)])
    out = render(2, 700.0, 64, sync_master_phase=master)
    assert np.isfinite(out).all() and np.abs(out).max() <= 1.5


def test_sync_really_restarts_the_slave_phase():
    # The master wraps once, between samples 31 and 32, at phase 1.0. Before it
    # the slave runs free, so both renders agree; after it the slave is back at
    # the start of its cycle and the saws must diverge (the free run is at phase
    # 0.467, the synced one near the -1 minimum).
    master = np.concatenate([np.linspace(0.0, 0.9, 32), np.linspace(0.0, 0.9, 32)])
    synced = render(2, 700.0, 64, sync_master_phase=master)
    free = render(2, 700.0, 64)
    assert np.allclose(synced[:32], free[:32], atol=1e-12)
    assert np.abs(synced[32:] - free[32:]).max() > 0.5
    assert synced[32:].min() < -0.95
    assert np.abs(synced).max() <= 1.5


def test_sync_handles_several_wraps_in_one_block():
    # A master ramp that wraps every ~22 samples, at a different fractional
    # position each time, resets the slave repeatedly inside one block.
    master = (np.arange(96) * 0.045) % 1.0
    wraps = np.flatnonzero(np.diff(master) < 0) + 1
    assert len(wraps) >= 3
    synced = render(2, 500.0, 96, sync_master_phase=master)
    free = render(2, 500.0, 96)
    assert np.isfinite(synced).all()
    assert np.abs(synced).max() <= 1.5
    for i in wraps:
        assert synced[i] < free[i] - 0.2, i


def test_sync_keeps_every_wave_bounded():
    master = (np.arange(1024) * 0.17) % 1.0
    for wave in (0, 1, 2, 3):
        out = render(wave, 300.0, 1024, sync_master_phase=master)
        assert np.isfinite(out).all(), wave
        assert np.abs(out).max() <= 1.5, wave


def test_sync_master_phase_must_match_the_block():
    o = Oscillator(SR); o.wave = 2
    with pytest.raises(ValueError):
        o.render_into(np.zeros(16), 440.0, sync_master_phase=np.zeros(15))


def test_synced_render_reuses_its_block_buffers(monkeypatch):
    # Hard sync adds a scratch ramp and a wrap list, but it must not build new
    # block-sized buffers per call either: the engine renders every voice inside
    # the audio callback.
    o = Oscillator(SR); o.wave = 2
    buf = np.zeros(16)
    master = (np.arange(16) * 0.07) % 1.0
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1
            return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(20):
        o.render_into(buf, 440.0, sync_master_phase=master)
    assert calls["n"] == 0


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


def test_sync_wrap_on_block_boundary_is_not_missed():
    # Master wraps exactly between block 1 and block 2: 0.9 -> 0.05.
    m = np.concatenate([np.linspace(0.5, 0.9, 32), np.linspace(0.05, 0.45, 32)])
    whole = render(2, 700.0, 64, sync_master_phase=m)
    o = Oscillator(SR); o.wave = 2
    a = np.zeros(32); b = np.zeros(32)
    o.render_into(a, 700.0, sync_master_phase=m[:32])
    o.render_into(b, 700.0, sync_master_phase=m[32:])
    free = render(2, 700.0, 64)
    assert np.abs(b - free[32:]).max() > 0.3  # reset happened at sample 32
    assert np.abs(b[3:] - whole[35:]).max() < 1e-9
