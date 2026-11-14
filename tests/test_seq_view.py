import tkinter as tk

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
    v = SeqView(root, e.synth.sequencer, e.transport)
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


def test_play_button_toggles_shared_transport(view):
    v, e = view
    v.toggle_transport()
    assert e.transport.playing and e.drums.playing
    assert v.play_btn["text"] == "STOP"


def test_refresh_highlights_playhead_only_while_playing(view):
    v, e = view
    v.refresh()
    assert all(int(str(b["highlightthickness"])) == 2 for b in v.steps)
    assert all(str(b["highlightbackground"]) != "#ffffff" for b in v.steps)
    e.transport.start()
    v.refresh()
    assert str(v.steps[0]["highlightbackground"]) == "#ffffff"


def test_help_exists():
    for k in ("seq_toggle", "seq_steps", "seq_gate", "seq_transport"):
        assert help_for(k)[0]


def test_gate_slider_writes_sequencer(view):
    v, e = view
    v.gate.set(80)
    v.update()
    assert e.synth.sequencer.gate == pytest.approx(0.8)
