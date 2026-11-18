import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.engine.params import REGISTRY, clamp
from nomad.engine.voice import Voice
from nomad.engine.voice_manager import VoiceManager

SR = 48000.0


def params(**over):
    p = {k: v.default for k, v in REGISTRY.items()}
    p.update(over)
    return p


def mods():
    return dict.fromkeys(("pitch", "pitch1", "pitch2", "amp", "cutoff", "pw1", "pw2", "bend"), 0.0)


def run(v, p, frames, block=64):
    out = np.zeros(block)
    for _ in range(frames // block):
        out[:] = 0
        v.render(out, p, mods())


def freq(sig):
    """Dominant frequency by zero crossings (clean saw/sine-ish test tone)."""
    s = np.sign(sig - sig.mean())
    return np.count_nonzero(np.diff(s) > 0) * SR / len(sig)


def test_glide_param_range_and_default():
    assert REGISTRY["glide"].default == 0.0
    assert clamp("glide", 5000) == 1000.0 and clamp("glide", float("nan")) == 0.0


def test_no_glide_pitch_is_the_note_exactly():
    v = Voice(SR, 0)
    v.note_on(72, 1.0, 60.0)  # a start pitch is ignored when glide is off
    run(v, params(), 64)
    assert v._cur == 72.0


def test_fresh_voice_glides_from_start_to_note():
    v = Voice(SR, 0)
    v.note_on(72, 1.0, 60.0)
    p = params(glide=200.0)
    run(v, p, 64)
    assert 60.0 < v._cur < 72.0  # moving, not jumped
    run(v, p, int(SR * 0.25))
    assert v._cur == pytest.approx(72.0, abs=0.4)  # ~3 time constants: within a few %
    run(v, p, int(SR * 0.5))
    assert v._cur == 72.0  # snapped to the target


def test_steal_continues_from_the_old_pitch():
    v = Voice(SR, 0)
    v.note_on(60, 1.0)
    p = params(glide=300.0, amp_attack=1.0)
    run(v, p, 2048)
    v.begin_steal(72, 1.0)
    run(v, p, int(SR * 0.02))  # past the 5 ms steal fade: the new note is triggered
    assert v._pitch_note == 72 and v._cur < 64.0  # starts from the old pitch, not from 72


def test_audio_pitch_really_slides():
    v = Voice(SR, 0)
    p = params(glide=300.0, osc1_wave=0, osc2_level=0.0, osc1_level=1.0, filt_cutoff=18000.0, amp_attack=1.0)
    v.note_on(81, 1.0, 69.0)  # A4 -> A5
    chunks = []
    for _ in range(int(SR * 0.6) // 256):
        o = np.zeros(256)
        v.render(o, p, mods())
        chunks.append(o)
    sig = np.concatenate(chunks)
    early, late = freq(sig[:2400]), freq(sig[-4800:])
    assert 430 < early < 700 and late == pytest.approx(880, rel=0.05)


def test_poly_chord_voices_all_converge():
    vm = VoiceManager(SR, 4)
    vm.set_glide(150.0)
    vm.note_on(48, 1.0)  # previous chord's last note
    vm.note_off(48)
    for n in (60, 64, 67):
        vm.note_on(n, 1.0)
    p = params(glide=150.0)
    out = np.zeros(256)
    for _ in range(int(SR * 0.6) // 256):
        vm.render(out, p, mods())
    cur = sorted(v._cur for v in vm.voices if v.is_active() and v.note in (60, 64, 67))
    assert cur == pytest.approx([60.0, 64.0, 67.0], abs=0.01)


def test_mono_legato_does_not_retrigger_or_add_voices():
    vm = VoiceManager(SR, 4)
    vm.set_polyphony(1)
    vm.set_glide(100.0)
    p = params(glide=100.0, amp_attack=1.0, amp_decay=2000.0, amp_sustain=0.7)
    vm.note_on(60, 1.0)
    out = np.zeros(256)
    for _ in range(40):
        vm.render(out, p, mods())
    v = vm.voices[0]
    lvl = v.amp_env.value
    vm.note_on(67, 1.0)  # key 60 still held: legato
    assert sum(x.is_active() for x in vm.voices) == 1 and v._pending is None and v._fade_left == 0
    vm.render(out, p, mods())
    assert v.amp_env.value == pytest.approx(lvl, abs=0.05) and v.note == 67 and v._pitch_note == 67
    vm.note_off(67)
    assert vm.active_notes() == []


def test_mono_without_glide_still_steals_as_before():
    vm = VoiceManager(SR, 4)
    vm.set_polyphony(1)
    vm.note_on(60, 1.0)
    vm.note_on(67, 1.0)
    assert vm.voices[0]._pending == (67, 1.0)


def test_engine_glide_param_reaches_the_voice_manager_and_renders_clean():
    e = Engine(48000)
    s = e.synth
    s.set_param("polyphony", 1)
    s.set_param("glide", 150.0)
    out = np.zeros(4800)
    e.render(out)
    assert s.vm.glide == 150.0
    e.note_on(60, 1.0)
    e.render(out)
    e.note_on(72, 1.0)
    big = np.zeros(48000)
    e.render(big)
    assert np.isfinite(big).all() and np.abs(big).max() > 0.01
