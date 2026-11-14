import numpy as np
import pytest

from nomad.engine.engine import CONTROL_BLOCK, Engine

SR = 48000.0


def run(e, n=int(SR), size=256):
    out = np.zeros(n)
    for i in range(0, n, size):
        e.render(out[i : i + size])
    return out


def test_quantum_agnostic_render():
    def render(e, sizes):
        out = np.zeros(sum(sizes)); i = 0
        for n in sizes:
            e.render(out[i : i + n]); i += n
        return out

    def make():
        e = Engine(SR)
        e.set_param("amp_attack", 1.0); e.set_param("filt_cutoff", 3000.0)
        e.note_on(60, 1.0)
        return e

    assert CONTROL_BLOCK == 256
    a = render(make(), [2048])
    b = render(make(), [256] * 8)
    assert np.allclose(a, b, atol=1e-6)
    c = render(make(), [37, 1, 128, 500, 88, 1294])
    assert np.isfinite(c).all()
    assert np.abs(np.diff(c)).max() < 0.2


def test_render_does_not_allocate(monkeypatch):
    e = Engine(SR); e.note_on(60, 1.0)
    out = np.zeros(256)
    e.render(out)
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1; return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(20): e.render(out)
    assert calls["n"] == 0


def test_eight_voices_stay_below_full_scale():
    e = Engine(SR); e.set_param("master_volume", 1.0)
    for n in range(48, 56): e.note_on(n, 1.0)
    out = run(e)
    assert np.abs(out).max() <= 1.0 and np.isfinite(out).all()


def test_extreme_params_stay_finite():
    e = Engine(SR)
    e.set_param("filt_cutoff", 20.0); e.set_param("filt_reso", 1.0)
    e.set_param("amp_attack", 1.0); e.set_param("amp_release", 8000.0)
    for n in (36, 48, 60, 72, 84): e.note_on(n, 1.0)
    out = run(e)
    assert np.isfinite(out).all() and np.abs(out).max() <= 1.0


def test_panic_silences_within_50ms():
    e = Engine(SR); e.note_on(60, 1.0)
    e.render(np.zeros(4096))
    e.panic()
    tail = np.zeros(2400)
    for i in range(0, len(tail), 256): e.render(tail[i : i + 256])
    assert np.abs(tail[-256:]).max() < 1e-3
    assert e.snapshot()["active_notes"] == []


def test_pitch_bend_shifts_pitch():
    def crossings(e):
        return int(np.sum(np.diff(np.signbit(run(e))) != 0))
    a = Engine(SR); b = Engine(SR)
    for e in (a, b):
        e.set_param("osc1_wave", 0); e.set_param("osc2_level", 0.0)
        e.set_param("filt_cutoff", 18000.0); e.note_on(57, 1.0)
    b.set_param("bend_range", 2.0); b.set_control("bend", 1.0)
    assert crossings(b) > crossings(a) * 1.08


def test_modwheel_reaches_matrix():
    def go(modwheel):
        e = Engine(SR)
        e.set_param("mod0_src", 3); e.set_param("mod0_dst", 4)
        e.set_param("mod0_amt", -1.0); e.set_param("amp_attack", 1.0)
        e.set_param("amp_sustain", 1.0); e.note_on(60, 1.0)
        if modwheel: e.set_control("modwheel", 1.0)
        return np.abs(run(e)[24000:]).max()
    loud, muted = go(False), go(True)
    assert loud > 0.01 and muted < loud * 0.2


def test_unknown_param_raises():
    with pytest.raises(KeyError): Engine(SR).set_param("nope", 1.0)


# ---- extras -----------------------------------------------------------------
def test_unknown_control_raises_on_caller_thread():
    with pytest.raises(KeyError): Engine(SR).set_control("nope", 1.0)


def test_bad_commands_never_break_render():
    e = Engine(SR)
    e.note_on(999, 1.0); e.note_on(None, 1.0); e.note_on(60, float("nan")); e.note_off(-5)
    e.set_param("filt_cutoff", float("nan")); e.set_param("mod3_src", 99); e.set_param("mod3_dst", -4)
    e.set_control("bend", float("nan"))
    out = run(e, 2048)
    assert np.isfinite(out).all()
    assert e.snapshot()["filt_cutoff"] == 1200.0  # NaN -> default
    assert e.snapshot()["mod3_src"] == 5.0  # clamped to the registry range


def test_handler_exception_is_swallowed(monkeypatch):
    e = Engine(SR)
    monkeypatch.setattr(e.matrix, "set_slot", lambda *a: (_ for _ in ()).throw(ValueError("x")))
    e.set_param("mod0_src", 1)
    e.render(np.zeros(256))
    assert e.errors == 1


def test_mod_slot_params_forwarded():
    e = Engine(SR)
    e.set_param("mod2_src", 1); e.set_param("mod2_dst", 5); e.set_param("mod2_amt", 0.5)
    e.render(np.zeros(16))
    assert (e.matrix._src[2], e.matrix._dst[2], e.matrix._amt[2]) == (1, 5, 0.5)


def test_polyphony_param_limits_voices():
    e = Engine(SR); e.set_param("polyphony", 2)
    for n in (60, 62, 64): e.note_on(n, 1.0)
    run(e, 2048)
    assert sum(v.is_active() for v in e.vm.voices) == 2
    assert e.vm.polyphony == 2


def test_snapshot_is_a_copy_with_controls_and_notes():
    e = Engine(SR)
    e.set_param("master_volume", 0.25); e.set_control("bend", 5.0); e.set_control("modwheel", -3.0)
    e.note_on(60, 1.0); e.note_on(64, 1.0); e.render(np.zeros(256))
    s = e.snapshot()
    assert s["master_volume"] == 0.25 and s["bend"] == 1.0 and s["modwheel"] == -1.0
    assert sorted(s["active_notes"]) == [60, 64]
    s["master_volume"] = 0.0
    assert e.snapshot()["master_volume"] == 0.25
    assert all(n in s for n in ("osc1_wave", "mod7_amt", "bend_range"))


def test_zero_and_empty_render():
    e = Engine(SR); e.note_on(60, 1.0)
    e.render(np.zeros(0))
    out = np.zeros(1); e.render(out)
    assert np.isfinite(out).all()


def test_master_volume_jump_has_no_click():
    e = Engine(SR); e.set_param("master_volume", 0.0)
    e.set_param("osc1_wave", 0); e.set_param("osc2_level", 0.0); e.set_param("filt_cutoff", 18000.0)
    e.set_param("amp_sustain", 1.0); e.note_on(60, 1.0)
    run(e, 9600)
    e.set_param("master_volume", 1.0)
    out = run(e, 9600)
    assert np.abs(np.diff(out)).max() < 0.05  # a 220 Hz sine at full scale steps ~0.03


def test_cutoff_jump_has_no_click():
    def go(jump):
        e = Engine(SR); e.set_param("filt_cutoff", 200.0 if jump else 12000.0); e.set_param("filt_reso", 0.0)
        e.set_param("amp_sustain", 1.0); e.note_on(48, 1.0)
        run(e, 9600)
        e.set_param("filt_cutoff", 12000.0)
        return np.abs(np.diff(run(e, 4800))).max()
    # the saw's own edge is the loudest step; the jump must not exceed it by >20%
    assert go(True) < go(False) * 1.2


def test_lfo_params_reach_lfos_and_pitch_slot_changes_pitch():
    e = Engine(SR)
    e.set_param("lfo1_rate", 5.0); e.set_param("lfo1_wave", 3); e.set_param("lfo1_depth", 0.5)
    e.set_param("lfo1_phase", 0.25)
    run(e, 24000)
    lf = e.lfos[0]
    assert (lf._rate, lf._wave, lf._offset) == (pytest.approx(5.0, rel=0.1), 3, 0.25)

    def crossings(depth):
        e = Engine(SR)
        e.set_param("osc1_wave", 0); e.set_param("osc2_level", 0.0); e.set_param("filt_cutoff", 18000.0)
        e.set_param("lfo1_wave", 3); e.set_param("lfo1_rate", 0.02); e.set_param("lfo1_depth", depth)
        e.set_param("lfo1_phase", 0.0)
        e.set_param("mod0_src", 1); e.set_param("mod0_dst", 1); e.set_param("mod0_amt", 1.0)
        e.note_on(57, 1.0)
        return int(np.sum(np.diff(np.signbit(run(e))) != 0))
    assert crossings(0.5) > crossings(0.0) * 1.3  # +6 st square LFO at its positive half


def test_velocity_modulation_is_per_voice():
    e = Engine(SR)
    e.set_param("mod0_src", 4); e.set_param("mod0_dst", 4); e.set_param("mod0_amt", 1.0)
    e.note_on(60, 0.2); e.note_on(72, 1.0)
    e.render(np.zeros(256))
    seen = []
    orig = e._resolve
    def spy(v, m):
        r = orig(v, m); seen.append(r["amp"]); return r
    e.vm.render(np.zeros(256), e._p, e._mods, spy)
    assert len(set(seen)) == 2 and sorted(seen) == pytest.approx([0.2, 1.0])
