import pytest

from nomad.engine.clock import STEPS, Clock, StepGrid, StepTracker

SR = 48000.0


def clock(bpm=120.0):
    c = Clock(SR)
    c.bpm = bpm
    return c


def test_step_grid_even_odd_lengths():
    g = StepGrid(swing=0.5)
    assert g.step_beats(0) == pytest.approx(0.375) and g.step_beats(1) == pytest.approx(0.125)
    assert g.step_at(0.0) == 0 and g.step_at(0.374) == 0 and g.step_at(0.376) == 1 and g.step_at(0.5) == 2


def test_clock_advance_120bpm():
    c = clock()
    c.start()
    c.advance(6000)
    assert c.pos == pytest.approx(0.25)


def test_stopped_clock_does_not_advance_and_frames_to_is_far():
    c = clock()
    c.advance(6000)
    assert c.pos == 0.0 and c.frames_to(1.0) == 1 << 30


def test_frames_to_next_boundary_exact():
    c, g = clock(), StepGrid()
    c.start()
    assert c.frames_to(g.next_boundary(c.pos)) == 6000
    c.advance(6000)
    assert g.step_at(c.pos) == 1 and c.frames_to(g.next_boundary(c.pos)) == 6000


def test_bpm_jump_mid_step_does_not_skip():
    c, g = clock(60.0), StepGrid()
    c.start()
    c.advance(11040)
    assert g.step_at(c.pos) == 0
    c.bpm = 180.0
    c.advance(384)
    assert g.step_at(c.pos) == 1


def test_swing_jump_mid_step_moves_the_boundary_without_skipping():
    c, g = clock(120.0), StepGrid()
    c.start()
    c.advance(5000)
    g.swing = 0.6                      # step 0 now lasts 9600 frames, not 6000
    c.advance(1800)
    assert g.step_at(c.pos) == 0
    c.advance(2800)                    # 5000 + 1800 + 2800 = 9600
    assert g.step_at(c.pos) == 1


def test_tracker_swing_drag_never_repeats_a_step():
    c, g = clock(120.0), StepGrid()
    t = StepTracker(g)
    c.start()
    fired = []
    c.advance(7000)
    g.swing = 0.0
    t.poll(c)                          # joins mid step, silent
    for _ in range(40):
        c.advance(500)
        g.swing = 0.6 if g.swing == 0.0 else 0.0
        s = t.poll(c)
        if s is not None:
            fired.append(s)
    assert fired == sorted(set(fired))


def test_pattern_step_wraps_at_length():
    assert StepGrid(length=12).pattern_step(13) == 1 and STEPS == 16


def test_tracker_fires_start_then_each_boundary_once():
    c, t = clock(), StepTracker(StepGrid())
    c.start()
    fired = []
    for _ in range(3):
        s = t.poll(c)
        if s is not None:
            fired.append(s)
        assert t.poll(c) is None  # polling twice never double fires
        c.advance(t.frames_to_event(c))
    assert fired == [0, 1, 2]


def test_tracker_join_mid_step_waits_for_next_boundary():
    c, t = clock(), StepTracker(StepGrid())
    c.start()
    c.advance(1000)
    assert t.poll(c) is None
    c.advance(t.frames_to_event(c))
    assert t.poll(c) == 1


def test_tracker_restart_refires_step0_and_stop_resets():
    c, t = clock(), StepTracker(StepGrid())
    c.start()
    c.advance(7000)
    t.poll(c)
    c.stop()
    assert t.poll(c) is None and t.last == -1
    c.start()
    assert t.poll(c) == 0
