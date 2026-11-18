import tkinter as tk

import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.gui.app import build_app
from nomad.gui.help import help_for


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


def tick(app, frames=64):
    app.engine.render(np.zeros(frames))
    for s in app.strips.values():
        s.refresh()
    app.root.update()


def names(opts):
    return ["Internal" if o is None else o.name for o in opts]


def test_clock_menu_offers_only_internal_other_instances(app):
    s1, d1 = app.engine.synth, app.engine.drums
    s2 = app.add_instrument("synth", defer=False)
    app.manager.set_source(d1, s1)
    assert names(app.strips[s2.id].source_options()) == ["Internal", "Synth 01"]
    assert names(app.strips[s1.id].source_options()) == ["Internal"]          # a master cannot follow
    assert names(app.strips[d1.id].source_options()) == ["Internal", "Synth 01", "Synth 02"]


def test_following_locks_bpm_and_play(app):
    s1, d1 = app.engine.synth, app.engine.drums
    app.manager.set_bpm(s1, 128.0)
    app.strips[d1.id].cycle_source()                    # Internal -> Synth 01
    tick(app)
    assert d1.source is s1
    st = app.strips[d1.id]
    assert str(st.play_btn["state"]) == "disabled" and str(st.bpm["state"]) == "disabled"
    assert st._bpm_var.get() == pytest.approx(128.0) and d1.values["bpm"] == pytest.approx(128.0)
    s2 = app.add_instrument("synth", defer=False)
    app.strips[s2.id].cycle_source()
    tick(app)
    st = app.strips[s2.id]
    assert str(st.bpm["state"]) == "disabled" and str(st.play_btn["state"]) == "disabled"
    assert st.clock_btn["text"] == "CLOCK: Synth 01"


def test_follower_shows_master_bpm(app):
    s1 = app.engine.synth
    s2 = app.add_instrument("synth", defer=False)
    app.strips[s2.id].cycle_source()
    app.manager.set_bpm(s1, 140.0)
    tick(app)
    tick(app)
    assert app.strips[s2.id]._bpm_var.get() == pytest.approx(140.0)
    assert not app.manager.set_bpm(s2, 90.0)            # ignored for a follower
    tick(app)
    assert s2.eclock.bpm == pytest.approx(140.0)


def test_going_internal_unlocks_and_keeps_bpm(app):
    s1 = app.engine.synth
    s2 = app.add_instrument("synth", defer=False)
    app.manager.set_bpm(s1, 133.0)
    app.strips[s2.id].cycle_source()
    tick(app)
    app.manager.set_source(s2, None)                    # back to Internal
    tick(app)
    st = app.strips[s2.id]
    assert s2.source is None and str(st.bpm["state"]) == "normal"
    assert st.clock_btn["text"] == "CLOCK: Internal" and s2.clock.bpm == pytest.approx(133.0)
    assert app.manager.set_bpm(s2, 90.0)


def test_closing_master_updates_follower_ui_to_internal(app):
    s1, d1 = app.engine.synth, app.engine.drums
    app.manager.set_bpm(s1, 125.0)
    app.manager.start(s1)
    app.strips[d1.id].cycle_source()
    tick(app, 4800)
    app.close_instance(s1)
    tick(app)
    assert d1.source is None and d1.clock.running and d1.clock.bpm == pytest.approx(125.0)
    assert app.strips[d1.id].clock_btn["text"] == "CLOCK: Internal"
    assert str(app.strips[d1.id].bpm["state"]) == "normal"
    assert d1.values["bpm"] == pytest.approx(125.0)     # the drum knob keeps the inherited tempo


def test_mute_and_level_controls_call_manager(app):
    s1 = app.engine.synth
    st = app.strips[s1.id]
    st.toggle_mute()
    st.level.set(50)
    app.root.update()
    tick(app)
    assert s1._muted and s1._level == pytest.approx(0.5)
    st.toggle_mute()
    tick(app)
    assert not s1._muted


def test_help_for_new_controls():
    for k in ("clock_source", "inst_mute", "inst_level", "inst_bpm", "tab_select", "tab_close", "tab_new"):
        assert help_for(k)[0]


def test_window_still_fits_with_strip(app):
    app.root.update_idletasks()
    assert app.root.winfo_reqwidth() <= 1366 and app.root.winfo_reqheight() <= 768


def test_bar_play_bpm_and_visibility_for_both_kinds(app):
    s1, d1 = app.engine.synth, app.engine.drums
    st_s, st_d = app.strips[s1.id], app.strips[d1.id]
    app.select(s1)
    app.root.update()
    assert st_s.winfo_ismapped() and not st_d.winfo_ismapped()
    app.select(d1)
    app.root.update()
    assert st_d.winfo_ismapped() and not st_s.winfo_ismapped()
    for inst, st in ((s1, st_s), (d1, st_d)):
        st.play_btn.invoke()
        tick(app)
        assert inst.eclock.running and st.play_btn["text"] == "STOP"
        st._bpm_var.set(133)
        st._on_bpm()
        tick(app)
        assert inst.eclock.bpm == pytest.approx(133.0)
        st.play_btn.invoke()
        tick(app)
        assert not inst.eclock.running
    assert d1.values["bpm"] == pytest.approx(133.0)
