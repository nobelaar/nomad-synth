import tkinter as tk

import pytest

from nomad.engine.engine import Engine
from nomad.engine.instrument import SynthInstrument
from nomad.gui.app import build_app
from nomad.gui.synth_panel import LAYOUT, SynthPanel


@pytest.fixture
def app():
    try:
        a = build_app(Engine(48000))
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    yield a
    a.close()


def test_panel_widgets_bound_to_its_own_instrument(app):
    other = SynthInstrument(48000, name="Synth 02", iid=9)
    p2 = SynthPanel(app.root, app, other)
    p2.widgets["filt_reso"].on_change(0.77)
    assert other.snapshot()["filt_reso"] == pytest.approx(0.77)
    assert app.engine.synth.snapshot()["filt_reso"] != pytest.approx(0.77)
    app.synth_view.widgets["filt_cutoff"].on_change(3000.0)
    assert other.snapshot()["filt_cutoff"] != pytest.approx(3000.0)


def test_master_volume_not_in_panels_but_voice_params_are(app):
    assert "master_volume" not in app.synth_view.widgets
    assert {"polyphony", "bend_range"} <= set(app.synth_view.widgets)
    assert "master_volume" in app.widgets and "master_volume" in LAYOUT["global"]


def test_app_widgets_alias_selected_panel(app):
    assert app.widgets["filt_cutoff"] is app.synth_view.widgets["filt_cutoff"]
    app.widgets["master_volume"].on_change(0.31)
    assert app.engine.snapshot()["master_volume"] == pytest.approx(0.31)


def test_panel_help_registered(app):
    for name, w in app.synth_view.widgets.items():
        assert str(w) in app._help_keys, name


def test_panel_visibility_hides_side_section_too(app):
    app.synth_view.set_visible(False)
    app.root.update()
    assert not app.synth_view.winfo_ismapped() and not app.synth_view.side_frame.winfo_ismapped()
    app.synth_view.set_visible(True)
    app.root.update()
    assert app.synth_view.winfo_ismapped() and app.synth_view.side_frame.winfo_ismapped()


def test_matrix_is_collapsed_by_default_and_counts_active_slots(app):
    p = app.synth_view
    app.root.update()
    assert not p._matrix_body.winfo_ismapped() and "0 en uso" in p.mod_btn["text"]
    app.engine.synth.set_param("mod0_src", 1)
    app.engine.synth.set_param("mod0_dst", 5)
    app.engine.synth.set_param("mod0_amt", 0.3)
    p.refresh()
    assert "1 en uso" in p.mod_btn["text"]
    p.mod_btn.invoke()
    app.root.update()
    assert p._matrix_body.winfo_ismapped() and "▾" in p.mod_btn["text"]
    assert "mod0_src" in p.widgets  # widgets exist even while collapsed
