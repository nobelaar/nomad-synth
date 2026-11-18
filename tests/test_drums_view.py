import tkinter as tk

import pytest

from nomad.engine.drums import DrumMachine
from nomad.gui.drums_view import PLAYHEAD, DrumView


@pytest.fixture
def view():
    try:
        root = tk.Tk()
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    dm = DrumMachine(48000)
    v = DrumView(root, dm)
    root.update()
    yield v, dm
    root.destroy()


def test_click_toggles_and_shift_accents(view):
    v, dm = view
    v.toggle_step("kick", 0, accent=False)
    assert dm.steps[0, 0] == 0          # el patrón inicial lo tenía prendido
    v.toggle_step("kick", 0, accent=False)
    assert dm.steps[0, 0] == 1
    v.toggle_step("kick", 1, accent=True)
    assert dm.steps[0, 1] == 2
    v.toggle_step("kick", 1, accent=True)
    assert dm.steps[0, 1] == 0


def test_refresh_marks_playhead(view):
    v, dm = view
    dm.start()
    v.refresh()
    assert v._steps[0][0].cget("highlightbackground") == PLAYHEAD


def test_knobs_write_through_to_the_machine(view):
    v, dm = view
    v.knobs["kick_level"].on_change(0.2)
    assert dm.values["kick_level"] == pytest.approx(0.2)


def test_reset_returns_a_knob_to_its_default(view):
    v, dm = view
    v.set_value("kick_level", 0.1)
    v.reset("kick_level")
    assert dm.values["kick_level"] == pytest.approx(0.6)


def test_clear(view):
    v, dm = view
    v.clear()
    assert not dm.steps.any()


def test_fits_in_the_panel(view):
    v, _ = view
    assert v.winfo_reqwidth() <= 1100 and v.winfo_reqheight() <= 700
