import tkinter as tk
from types import SimpleNamespace

import pytest

from nomad.engine.engine import Engine
from nomad.engine.params import REGISTRY
from nomad.gui.app import HIDDEN, LAYOUT, build_app
from nomad.gui.help import help_for


def test_every_visible_parameter_and_element_has_help():
    for name in {p for v in LAYOUT.values() for p in v}:
        title, body = help_for(name)
        assert title and len(body) > 40, name
    for key in ("help", "learn", "audio", "midi", "voices", "xruns", "keyboard", "scope"):
        assert help_for(key)[0]
    assert set(REGISTRY) - HIDDEN <= {p for v in LAYOUT.values() for p in v}
    with pytest.raises(KeyError):
        help_for("nope")


def test_matrix_destinations_explain_p1_p2_as_oscillator_pitch():
    body = help_for("mod0_dst")[1]
    assert "P1: afinación solo del oscilador 1" in body and "P2: afinación solo del oscilador 2" in body


@pytest.fixture
def app():
    try:
        a = build_app(Engine(48000))
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    a.root.update()
    yield a
    a.close()


def test_help_mode_cursor_pauses_controls_and_restores(app):
    knob = app.widgets["filt_cutoff"]
    sel_btn = app.widgets["osc1_wave"]._btns[0]
    app.toggle_help()
    assert app.help_on and str(knob["cursor"]) == "question_arrow"
    assert str(app.help_btn["cursor"]) != "question_arrow"  # the way out stays clickable
    before = knob.model.value
    knob.event_generate("<Button-4>", x=5, y=5)  # wheel up
    app.root.update()
    assert knob.model.value == before
    app.widgets["osc1_wave"].set_index(2)
    sel_btn.event_generate("<ButtonPress-1>", x=2, y=2)
    sel_btn.event_generate("<ButtonRelease-1>", x=2, y=2)
    app.root.update()
    assert app.widgets["osc1_wave"].index == 2
    app.toggle_help()
    assert not app.help_on and str(knob["cursor"]) == "sb_v_double_arrow"
    knob.event_generate("<Button-4>", x=5, y=5)
    app.root.update()
    assert knob.model.value > before


def test_hover_shows_the_explanation_window(app):
    app.toggle_help()
    child = app.widgets["mod2_dst"]._btns[1]
    assert app._help_key_for(child) == "mod2_dst"
    app._help_show("mod2_dst", child)
    app.root.update()
    assert app._help_win.winfo_ismapped()
    assert app._help_title["text"] == "Destino de modulación — slot 3"
    assert "PIT" in app._help_body["text"]
    x = app._help_win.winfo_rootx() + app._help_win.winfo_width()
    assert x <= app.root.winfo_screenwidth()
    app._help_enter(SimpleNamespace(widget=app.widgets["filt_reso"]))  # schedules, must not raise
    app.toggle_help()
    app.root.update()
    assert not app._help_win.winfo_ismapped()


def test_every_drum_control_has_help():
    from nomad.engine.drums import DRUM_REGISTRY
    for name in DRUM_REGISTRY:
        title, body = help_for(name)
        assert title and len(body) > 40, name
    for key in ("drums_view", "drum_steps", "drum_transport", "synth_view"):
        assert help_for(key)[0]
