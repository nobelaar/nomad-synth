import pytest

from nomad.engine.transport import Transport

SR = 48000.0


def make(bpm=120.0):
    t = Transport(SR)
    t.bpm = bpm
    return t


def run(t, sizes, limit=None):
    spans, steps = [], []
    for n in sizes:
        t.run(n, lambda s, c: spans.append(c), steps.append, limit)
    return spans, steps


def test_step_frames_at_120bpm():
    assert make().step_frames(0) == 6000


def test_swing_alternates():
    t = make()
    t.swing = 0.5
    assert t.step_frames(0) == 9000 and t.step_frames(1) == 3000


def test_run_fires_step0_on_start_then_every_6000_frames():
    t = make()
    t.start()
    spans, steps = run(t, [12001])
    assert steps == [0, 1, 2]
    assert max(spans) <= 6000 and sum(spans) == 12001


def test_stopped_run_is_one_span_no_events():
    t = make()
    spans, steps = run(t, [500])
    assert spans == [500] and steps == []


def test_bpm_jump_mid_step_does_not_skip():
    t = make(60.0)
    t.start()
    run(t, [11040])
    assert t.position == 0
    t.bpm = 180.0
    run(t, [384])
    assert t.position == 1


def test_swing_jump_mid_step_does_not_skip_steps():
    t = make(120.0)
    t.start()
    run(t, [5000])
    t.swing = 0.6
    run(t, [1800])
    assert t.position == 1


def test_limit_caps_spans():
    t = make()
    t.start()
    spans, _ = run(t, [1000], limit=lambda: 100)
    assert max(spans) == 100 and sum(spans) == 1000
