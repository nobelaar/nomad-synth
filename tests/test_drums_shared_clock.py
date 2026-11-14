import numpy as np

from nomad.engine.clock import Clock
from nomad.engine.drums import DrumMachine

SR = 48000.0


def test_dm_owns_clock_by_default():
    assert DrumMachine(SR).clock is not None


def test_two_drum_machines_share_one_clock():
    c = Clock(SR)
    c.bpm = 120.0
    a, b = DrumMachine(SR, clock=c), DrumMachine(SR, clock=c)
    for d in (a, b):
        d.clear_pattern()
        d.set_step("chat", 0, 1)
        d._ensure(6000)
    c.start()
    oa, ob = np.zeros(6000), np.zeros(6000)
    i = 0
    while i < 6000:
        a.poll(), b.poll()
        n = min(6000 - i, a.frames_to_event(), b.frames_to_event())
        a.render_chunk(oa, i, n)
        b.render_chunk(ob, i, n)
        c.advance(n)  # advanced once, by whoever owns the clock
        i += n
    assert np.abs(oa).max() > 0 and np.array_equal(oa, ob)
