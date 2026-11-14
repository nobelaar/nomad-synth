import tracemalloc

import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.engine.sequencer import StepSequencer
from nomad.engine.clock import Clock


def make(notes=(64,), gate=0.5):
    e = Engine(48000)
    e.drums.clear_pattern()
    e.seq_clock.bpm = 120.0  # 6000 frames per step (the Synth owns its tempo now)
    sq = e.synth.sequencer
    sq.enabled, sq.gate = True, gate
    for i, n in enumerate(notes):
        sq.set_step(i, True, note=n)
    return e, sq


def render(e, *sizes):
    parts = []
    for n in sizes:
        o = np.zeros(n)
        e.render(o)
        parts.append(o)
    return np.concatenate(parts)


def test_note_on_at_exact_frame():
    e, _ = make((64, 65))
    e.seq_clock.start()
    render(e, 1)
    assert e.vm.active_notes() == [64]
    render(e, 37, 5961)  # 5999 frames: step 1 not reached
    assert 65 not in e.vm.active_notes()
    render(e, 1)
    assert e.vm.active_notes() == [65]


def test_gate_off_after_gate_fraction():
    e, _ = make()
    e.seq_clock.start()
    render(e, 2999)
    assert e.vm.active_notes() == [64]
    render(e, 1)
    assert e.vm.active_notes() == []


def test_two_steps_retrigger_legato_gate_1():
    e, _ = make((60, 62), gate=1.0)
    e.seq_clock.start()
    render(e, 6000)
    assert e.vm.active_notes() == [62]


def test_empty_pattern_is_silent():
    e, sq = make()
    sq.clear()
    e.seq_clock.start()
    y = render(e, 6000, 100)
    assert not y.any() and e.vm.active_notes() == []


def test_disabled_sequencer_is_silent_and_releases():
    e, sq = make()
    e.seq_clock.start()
    render(e, 100)
    assert e.vm.active_notes() == [64]
    sq.enabled = False
    render(e, 10)
    assert e.vm.active_notes() == []
    render(e, 6000)
    assert e.vm.active_notes() == []


def test_stop_releases_note():
    e, _ = make()
    e.seq_clock.start()
    render(e, 100)
    e.seq_clock.stop()
    render(e, 10)
    assert e.vm.active_notes() == []


def test_panic_releases_note():
    e, _ = make()
    e.seq_clock.start()
    render(e, 100)
    e.panic()
    render(e, 10)
    assert e.vm.active_notes() == []


def test_live_key_not_cut_by_gate_off():
    e, _ = make((64,))
    e.note_on(64, 1.0)
    e.seq_clock.start()
    render(e, 3500)
    assert 64 in e.vm.active_notes()


def test_midi_and_sequence_play_together():
    e, _ = make((64,))
    e.note_on(48, 1.0)
    e.seq_clock.start()
    render(e, 100)
    assert sorted(e.vm.active_notes()) == [48, 64]


def test_bpm_jump_mid_step_no_skipped_or_double_note():
    e, sq = make((60, 62, 64, 65))
    e.seq_clock.bpm = 60.0
    calls = []
    real = e.vm.note_on
    e.vm.note_on = lambda n, v: (calls.append(n), real(n, v))[1]
    e.seq_clock.start()
    render(e, 11040)
    e.seq_clock.bpm = 180.0
    render(e, 384)
    assert calls == [60, 62]
    render(e, 4000)
    assert calls == [60, 62, 64]


def test_out_of_range_inputs():
    e, sq = make()
    sq.set_step(99, True)
    sq.set_step(0, True, note=300, vel=0)
    sq.gate = float("nan")
    assert sq.note[0] == 127 and sq.vel[0] == 1 and sq.gate == 0.5
    e.seq_clock.start()
    render(e, 100)


def test_no_allocation_in_step_path():
    class VM:
        def note_on(self, n, v): pass
        def note_off(self, n): pass

    class Synth:
        vm = VM()
        _live = bytearray(128)
        eclock = Clock(48000)

    sq = StepSequencer(Synth(), 48000)
    sq.enabled = True
    sq.set_step(0, True)
    for _ in range(50):
        sq.on_step(0), sq.advance(3000)
    tracemalloc.start()
    a = tracemalloc.take_snapshot()
    for _ in range(1000):
        sq.on_step(0)
        sq.frames_to_off()
        sq.advance(3000)
    b = tracemalloc.take_snapshot()
    tracemalloc.stop()
    grown = sum(s.size_diff for s in b.compare_to(a, "filename") if "sequencer.py" in str(s.traceback))
    assert grown < 1024


@pytest.mark.parametrize("how", ["stop", "disable"])
def test_live_key_survives_stop_and_seq_off(how):
    e, sq = make((64,))
    e.note_on(64, 1.0)
    e.seq_clock.start()
    render(e, 100)
    e.seq_clock.stop() if how == "stop" else setattr(sq, "enabled", False)
    render(e, 10)
    assert 64 in e.vm.active_notes()
