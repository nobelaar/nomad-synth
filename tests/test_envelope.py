import numpy as np
from nomad.engine.envelope import Envelope

SR = 48000.0

def test_attack_reaches_one_at_attack_time():
    e = Envelope(SR); e.set_params(0.01, 0.05, 0.6, 0.2); e.note_on()
    out = np.zeros(int(SR * 0.02)); e.process(out)
    assert out.max() >= 0.99
    assert abs(int(np.argmax(out)) - int(SR * 0.01)) <= 2

def test_decays_to_sustain():
    e = Envelope(SR); e.set_params(0.005, 0.05, 0.4, 0.2); e.note_on()
    out = np.zeros(int(SR * 0.2)); e.process(out)
    assert abs(out[-1] - 0.4) < 0.01

def test_release_starts_from_current_level():
    e = Envelope(SR); e.set_params(0.1, 0.1, 0.8, 0.2); e.note_on()
    a = np.zeros(int(SR * 0.05)); e.process(a)
    level = a[-1]
    e.note_off()
    b = np.zeros(64); e.process(b)
    assert abs(b[0] - level) < 0.01

def test_release_goes_silent_and_idles():
    e = Envelope(SR); e.set_params(0.001, 0.01, 0.5, 0.05); e.note_on()
    e.process(np.zeros(2000)); e.note_off()
    out = np.zeros(4000); e.process(out)
    assert out[-1] < 0.001 and e.is_idle()

def test_sustain_zero_decays_to_zero():
    e = Envelope(SR); e.set_params(0.001, 0.02, 0.0, 0.05); e.note_on()
    out = np.zeros(int(SR * 0.1)); e.process(out)
    assert out[-1] < 0.001

def test_bad_params_are_clamped():
    e = Envelope(SR)
    e.set_params(float("nan"), -1.0, float("inf"), 0.0); e.note_on()
    out = np.zeros(512); e.process(out)
    assert np.isfinite(out).all() and out.max() <= 1.0
    e.note_off(); e.process(np.zeros(512)); assert e.is_idle()

def test_retrigger_continues_from_value_and_idle_note_off_noop():
    e = Envelope(SR); e.note_off(); assert e.is_idle()
    e.set_params(0.05, 0.05, 0.5, 0.2); e.note_on()
    a = np.zeros(1000); e.process(a); e.note_off(); e.process(np.zeros(500))
    lvl = e.value; e.note_on()
    b = np.zeros(8); e.process(b)
    assert abs(b[0] - lvl) < 0.01 and b[-1] >= b[0]


def test_repeated_identical_set_params_does_not_stall_attack():
    e = Envelope(SR); e.set_params(0.005, 0.05, 0.5, 0.2); e.note_on()
    out = np.zeros(256)
    for _ in range(40):
        e.set_params(0.005, 0.05, 0.5, 0.2)
        e.process(out)
    assert abs(out[-1] - 0.5) < 0.01
