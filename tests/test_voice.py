import numpy as np
from nomad.engine.voice import Voice
from nomad.engine.params import REGISTRY

SR = 48000.0

def params(**over):
    p = {k: v.default for k, v in REGISTRY.items()}
    p.update(over); return p

def mods(**over):
    d = {k: 0.0 for k in ("pitch","pitch1","pitch2","amp","cutoff","pw1","pw2","bend")}
    d.update(over); return d

def render_voice(v, n, p, m=None, block=16):
    out = np.zeros(n)
    for i in range(0, n, block):
        v.render(out[i:i+block], p, m or mods())
    return out

def test_silent_until_note_on():
    v = Voice(SR, 0)
    assert np.allclose(render_voice(v, 512, params()), 0.0)

def test_note_produces_bounded_audio():
    v = Voice(SR, 0); v.note_on(60, 0.8)
    out = render_voice(v, 24000, params(amp_attack=1.0, amp_decay=50.0, amp_sustain=0.8))
    assert np.abs(out).max() > 0.01
    assert np.abs(out).max() <= 1.0
    assert np.isfinite(out).all()

def test_two_oscillators_beat_when_detuned():
    # 10 cents de detune a 220 Hz => batido de ~1.27 Hz: la envolvente sube y baja
    v = Voice(SR, 0); v.note_on(57, 1.0)
    p = params(osc1_wave=0, osc2_wave=0, osc1_level=0.5, osc2_level=0.5,
               osc2_fine=10.0, amp_attack=1.0, amp_sustain=1.0, filt_cutoff=18000.0)
    out = render_voice(v, 144000, p)          # 3 s: varios ciclos de batido
    env = np.abs(out[9600:]).reshape(-1, 480).max(axis=1)
    assert env.std() > 0.01                      # hay batido
    assert env.mean() > 0.01

def test_amp_env_silences_voice_after_release():
    v = Voice(SR, 0); v.note_on(60, 1.0)
    render_voice(v, 4096, params(amp_release=20.0))
    v.note_off()
    out = render_voice(v, 24000, params(amp_release=20.0))
    assert out[-100:].max() < 1e-3
    assert not v.is_active()

# ---- additions -------------------------------------------------------------

def _rms(x): return float(np.sqrt(np.mean(x**2)))

BASE = dict(osc1_wave=0, osc2_wave=0, amp_attack=1.0, amp_sustain=1.0,
            filt_cutoff=18000.0, filt_reso=0.0)

def _note(**over):
    v = Voice(SR, 0); v.note_on(60, 1.0)
    return render_voice(v, 9600, params(**{**BASE, **over}), block=256)[2400:]

def test_each_oscillator_contributes_and_level_zero_silences_it():
    both = _rms(_note(osc1_level=0.5, osc2_level=0.5, osc2_coarse=7.0))
    only1 = _rms(_note(osc1_level=0.5, osc2_level=0.0))
    only2 = _rms(_note(osc1_level=0.0, osc2_level=0.5))
    assert only1 > 0.05 and only2 > 0.05
    assert both > only1 * 1.1
    assert _rms(_note(osc1_level=0.0, osc2_level=0.0)) < 1e-9

def test_filter_env_amount_opens_the_cutoff():
    def hf(amt):
        v = Voice(SR, 0); v.note_on(48, 1.0)
        p = params(osc1_wave=2, osc2_level=0.0, osc1_level=1.0, amp_attack=1.0,
                   amp_sustain=1.0, filt_cutoff=300.0, filt_reso=0.0,
                   filt_attack=1.0, filt_decay=5000.0, filt_sustain=1.0,
                   filt_env_amt=amt)
        out = render_voice(v, 4800, p, block=256)[2400:]
        return _rms(np.diff(out))                 # proxy de brillo
    assert hf(1.0) > 3.0 * hf(0.0)

def test_max_resonance_is_bounded_and_finite_for_seconds():
    for cutoff in (20.0, 440.0, 18000.0):
        v = Voice(SR, 0); v.note_on(36, 1.0)
        p = params(filt_cutoff=cutoff, filt_reso=1.0, amp_attack=1.0, amp_sustain=1.0)
        out = render_voice(v, 48000 * 3, p, block=256)
        assert np.isfinite(out).all() and np.abs(out).max() <= 3.0   # raw ladder at k=4.3; the engine soft-clips

def test_velocity_scales_amplitude():
    def peak(vel):
        v = Voice(SR, 0); v.note_on(60, vel)
        return np.abs(render_voice(v, 9600, params(**BASE), block=256)).max()
    assert peak(1.0) > 1.8 * peak(0.4) > 0.0

def test_sync_changes_the_spectrum_and_stays_finite():
    kw = dict(osc1_wave=2, osc2_wave=2, osc2_coarse=9.0, osc1_level=0.0, osc2_level=1.0,
              filt_cutoff=18000.0, amp_attack=1.0, amp_sustain=1.0, filt_reso=0.0)
    def run(sync):
        v = Voice(SR, 0); v.note_on(48, 1.0)
        return render_voice(v, 9600, params(osc2_sync=sync, **kw), block=256)
    a, b = run(0.0), run(1.0)
    assert np.isfinite(b).all() and np.abs(b).max() <= 1.5
    assert not np.allclose(a, b, atol=1e-3)

def test_mods_pitch_is_in_semitones_and_does_not_leak_the_dict():
    def zc(**m):
        v = Voice(SR, 0); v.note_on(60, 1.0)
        out = render_voice(v, 48000, params(**{**BASE, "osc2_level": 0.0}), mods(**m), block=256)
        return int(np.sum(np.diff(np.signbit(out)) != 0))
    assert abs(zc(pitch=12.0) / zc() - 2.0) < 0.05
    assert abs(zc(bend=12.0) / zc() - 2.0) < 0.05

def test_free_mode_ignores_the_key():
    def zc(note):
        v = Voice(SR, 0); v.note_on(note, 1.0)
        out = render_voice(v, 48000, params(**{**BASE, "osc1_mode": 1.0, "osc1_hz": 200.0,
                                               "osc2_level": 0.0}), block=256)
        return int(np.sum(np.diff(np.signbit(out)) != 0))
    assert zc(36) == zc(84) and abs(zc(60) - 400) < 6

def _steal_run(block, vel2=1.0, wave=0):
    v = Voice(SR, 0); v.note_on(60, 1.0)
    p = params(osc1_wave=wave, osc2_level=0.0, osc1_level=1.0, amp_attack=1.0,
               amp_sustain=1.0, filt_cutoff=18000.0, filt_reso=0.0)
    render_voice(v, 8192, p, block=256)
    out = np.zeros(4800)
    stolen = False
    for i in range(0, len(out), block):
        if i >= 480 and not stolen:
            v.begin_steal(72, vel2); stolen = True
        v.render(out[i:i+block], p, mods())
    return v, out

def test_steal_has_no_click_and_retriggers_new_note():
    for block in (16, 256):
        v, out = _steal_run(block)
        # full-scale sines at 261/523 Hz move <= 0.034/0.069 per sample; a click would be ~0.5+.
        assert np.abs(np.diff(out)).max() < 0.08
        assert v.note == 72 and v.is_active()
        assert np.abs(out[-480:]).max() > 0.3         # the new note is sounding

def test_steal_fades_to_silence_before_retrigger():
    v, out = _steal_run(16)
    assert abs(out[480 + 239]) < 0.01          # fade hits exactly zero at the switch
    assert np.abs(out[480:480 + 240]).max() > 0.5

def test_note_off_during_steal_goes_silent():
    v = Voice(SR, 0); v.note_on(60, 1.0)
    p = params(**BASE)
    render_voice(v, 4096, p)
    v.begin_steal(72, 1.0); v.note_off()
    out = render_voice(v, 9600, p)
    assert np.abs(out[-480:]).max() < 1e-3 and not v.is_active()

def test_nan_free_at_extreme_params():
    for cutoff in (20.0, 18000.0):
        for reso in (0.0, 1.0):
            for note in (0, 127):
                v = Voice(SR, 0); v.note_on(note, 1.0)
                p = params(filt_cutoff=cutoff, filt_reso=reso, filt_env_amt=1.0,
                           osc1_coarse=24.0, osc2_coarse=-24.0, osc2_sync=1.0,
                           osc1_pw=0.05, osc2_pw=0.95, osc1_wave=3, osc2_wave=3,
                           amp_attack=1.0, amp_release=5.0)
                m = mods(pitch=60.0, pitch1=-60.0, pw1=5.0, pw2=-5.0, cutoff=10.0, amp=3.0)
                out = render_voice(v, 12000, p, m, block=256)
                assert np.isfinite(out).all()
    v = Voice(SR, 0); v.note_on(60, 1.0)
    out = render_voice(v, 2048, params(filt_cutoff=float("nan"), osc1_level=float("nan")),
                       mods(pitch=float("nan"), cutoff=float("inf")), block=256)
    assert np.isfinite(out).all()

def test_render_accumulates_into_out():
    v = Voice(SR, 0); v.note_on(60, 1.0)
    out = np.full(256, 0.25)
    v.render(out, params(), mods())
    assert not np.allclose(out, 0.25)
