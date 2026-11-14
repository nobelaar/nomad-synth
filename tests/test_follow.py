import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.engine.manager import InstrumentManager

SR = 48000


def setup(bpm=128.0):
    """Machine Drum = master (hat on every step); Synth follows it, sequencer on every step."""
    e = Engine(SR)
    m = InstrumentManager(e)
    e.drums.clear_pattern()
    for s in range(16):
        e.drums.set_step("chat", s, 1)
    sq = e.synth.sequencer
    sq.enabled = True
    for s in range(16):
        sq.set_step(s, True, note=48 + s)
    m.set_bpm(e.drums, bpm)
    log = {"drums": [], "seq": [], "now": 0}
    od, os_, adv = e.drums.on_step, sq.on_step, e.drums.advance

    def drums_step(k):
        log["drums"].append((log["now"], k))
        od(k)

    def seq_step(k):
        log["seq"].append((log["now"], k))
        os_(k)

    def advance(n):
        log["now"] += n
        adv(n)

    e.drums.on_step, sq.on_step, e.drums.advance = drums_step, seq_step, advance
    return e, m, sq, log


def run(e, frames, size=4096):
    done = 0
    while done < frames:
        n = min(size, frames - done)
        e.render(np.zeros(n))
        done += n


def test_follower_steps_align_with_master():
    e, m, sq, log = setup()
    m.set_source(e.synth, e.drums)
    m.start(e.drums)
    run(e, 3 * 16 * 5625)                      # 3 bars at 128 BPM
    assert len(log["drums"]) >= 48 and log["drums"][:48] == log["seq"][:48]


def test_bpm_change_on_master_reaches_follower():
    e, m, sq, log = setup(120.0)
    m.set_source(e.synth, e.drums)
    m.start(e.drums)
    run(e, 20000)
    m.set_bpm(e.drums, 128.0)
    run(e, 60000)
    t = [x[0] for x in log["seq"]]
    assert abs((t[-1] - t[-2]) - 5625) <= 1
    assert [x[0] for x in log["drums"]] == t


def test_different_length_patterns_stay_aligned():
    e, m, sq, log = setup()
    sq.grid.length = 12                        # own length; same shared clock
    m.set_source(e.synth, e.drums)
    m.start(e.drums)
    run(e, 30 * 5625)
    assert [x[0] for x in log["seq"]] == [x[0] for x in log["drums"]][: len(log["seq"])]
    assert [x[1] for x in log["seq"]][:14] == [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 1]
    assert [x[1] for x in log["drums"]][12] == 12


def test_master_stop_stops_followers_and_releases_notes():
    e, m, sq, log = setup()
    m.set_source(e.synth, e.drums)
    m.start(e.drums)
    run(e, 7000)
    assert e.vm.active_notes() or len(log["seq"]) > 1
    m.stop(e.drums)
    run(e, 64)
    n = len(log["seq"])
    assert e.vm.active_notes() == []
    run(e, 20000)
    assert len(log["seq"]) == n


def test_follower_of_stopped_master_is_silent():
    e, m, sq, log = setup()
    m.set_source(e.synth, e.drums)
    run(e, 30000)
    assert log["seq"] == [] and e.vm.active_notes() == []


def test_join_mid_run_does_not_fire_current_step():
    e, m, sq, log = setup()
    m.start(e.drums)
    run(e, 1000)
    m.set_source(e.synth, e.drums)
    run(e, 100)
    assert log["seq"] == []                    # mid-step: wait for the boundary
    run(e, 5625)
    assert [k for _, k in log["seq"]] == [1]   # aligned with the master's step 1


def test_switching_source_never_restarts_the_pattern():
    e, m, sq, log = setup()
    m.start(e.drums)
    run(e, 1000)
    m.start(e.synth)                           # own clock runs 1000 frames behind the drums
    run(e, 3 * 5625 + 100)
    before = [k for _, k in log["seq"]]
    m.set_source(e.synth, e.drums)             # immediate switch (documented: may jump or repeat a step)
    run(e, 3 * 5625)
    after = [k for _, k in log["seq"]][len(before):]
    # documented: the switch is immediate and may skip/repeat one step (here step 3 is skipped: the master's
    # grid is at step 3 mid-way, so the follower joins at the next master boundary). Never a restart to 0.
    assert before == [0, 1, 2] and after[:3] == [4, 5, 6]


def test_mute_is_independent_of_follow():
    e, m, sq, log = setup()
    m.set_source(e.synth, e.drums)
    m.set_mute(e.synth, True)
    m.start(e.drums)
    run(e, 3 * 5625)
    assert len(log["seq"]) >= 3
    out = np.zeros(2048)
    e.render(out)
    assert np.abs(out).max() > 0  # the drums still sound; the muted synth adds nothing
    assert e.synth._muted and e.synth._igain == 0.0
