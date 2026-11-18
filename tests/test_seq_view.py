import tkinter as tk

import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.gui.help import help_for
from nomad.gui.seq_view import SeqView


@pytest.fixture
def view():
    try:
        root = tk.Tk()
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    e = Engine(48000)
    v = SeqView(root, e.synth)
    root.update()
    yield v, e
    root.destroy()


def test_toggle_step_writes_sequencer(view):
    v, e = view
    sq = e.synth.sequencer
    v.toggle_step(3)
    assert sq.on[3] == 1 and sq.vel[3] == 100
    v.toggle_step(3)
    assert sq.on[3] == 0


def test_wheel_changes_note_and_clamps_0_127(view):
    v, e = view
    sq = e.synth.sequencer
    v.set_note(0, 12)
    assert sq.note[0] == 72
    v.set_note(0, 500)
    assert sq.note[0] == 127
    v.set_note(0, -500)
    assert sq.note[0] == 0


def test_accent_sets_vel_127(view):
    v, e = view
    v.toggle_step(1, accent=True)
    assert e.synth.sequencer.on[1] == 1 and e.synth.sequencer.vel[1] == 127
    v.toggle_step(1, accent=True)
    assert e.synth.sequencer.on[1] == 0


def test_seq_button_toggles_enabled(view):
    v, e = view
    v.toggle_enabled()
    assert e.synth.sequencer.enabled
    v.toggle_enabled()
    assert not e.synth.sequencer.enabled


def test_refresh_highlights_playhead_only_while_playing(view):
    v, e = view
    v.refresh()
    assert all(int(str(b["highlightthickness"])) == 3 for b in v.steps)
    assert all(str(b["highlightbackground"]) != "#ffffff" for b in v.steps)
    e.seq_clock.start()
    v.refresh()
    assert str(v.steps[0]["highlightbackground"]) == "#ffffff"


def test_help_exists():
    for k in ("seq_toggle", "seq_steps", "seq_gate"):
        assert help_for(k)[0]


def test_gate_slider_writes_sequencer(view):
    v, e = view
    v.gate.set(80)
    v.update()
    assert e.synth.sequencer.gate == pytest.approx(0.8)


def test_rec_button_and_div_len_labels(view):
    v, e = view
    v.toggle_rec()
    assert v.rec_btn["text"] == "REC·STEP" and str(v.rest_btn["state"]) == "normal"
    assert v.rec_btn["text"] == "REC·STEP"
    v.cycle_div(); v.cycle_len()
    assert v.div_btn["text"] == "DIV 1/8" and v.len_btn["text"] == "LEN 1"


def test_steps_show_dot_when_off_and_chord_count_when_on(view):
    v, e = view
    sq = e.synth.sequencer
    v.refresh()
    assert v.steps[0].cget("text").startswith("·")
    sq.set_step(0, True, note=48)
    sq.add_note(0, 52)
    sq.add_note(0, 55)
    v.refresh()
    assert v.steps[0].cget("text") == "C3\n+2"
