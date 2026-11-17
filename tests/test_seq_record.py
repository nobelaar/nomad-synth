import numpy as np

from nomad.engine.engine import Engine


def make():
    e = Engine(48000)
    e.seq_clock.bpm = 120.0  # 6000 frames per 16th
    s = e.synth
    s.sequencer.on.fill(0)
    s.set_rec(True)
    return e, s, s.sequencer


def render(e, n):
    e.render(np.zeros(n))


def test_step_record_writes_and_advances_with_wrap():
    e, s, sq = make()
    for n in range(60, 77):  # 17 notes: wraps to step 1
        s.note_on(n, 0.8)
    render(e, 64)
    assert list(sq.note[:16]) == [76] + list(range(61, 76))
    assert sq.on.all() and sq.rec_step == 1


def test_rest_skips_a_step():
    e, s, sq = make()
    s.note_on(60, 1.0)
    s.rest()
    s.note_on(62, 1.0)
    render(e, 64)
    assert list(sq.on[:3]) == [1, 0, 1] and sq.note[2] == 62


def test_realtime_quantizes_to_nearest_step():
    e, s, sq = make()
    s.start()
    render(e, 6000 + 4000)  # 1.67 steps in: nearest is step 2
    s.note_on(65, 1.0)
    render(e, 64)
    assert sq.on[2] == 1 and sq.note[2] == 65 and sq.on.sum() == 1


def test_rec_off_does_not_write():
    e, s, sq = make()
    s.set_rec(False)
    s.note_on(60, 1.0)
    render(e, 64)
    assert sq.on.sum() == 0
