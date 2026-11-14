import time
import tkinter as tk

import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.gui.app import build_app


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


def labels(app):
    return [b.cget("text") for b, _ in app.tabbar.tabs.values()]


def render(app, n=64):
    app.engine.render(np.zeros(n))


def test_plus_menu_lists_registered_types(app):
    assert app.tabbar.menu_labels() == ["Synth", "Machine Drum"]


def test_new_tab_creates_instance_and_selects_it(app):
    inst = app.add_instrument("synth", defer=False)
    app.root.update()
    assert app.selected is inst and inst in app.manager.instances
    assert labels(app) == ["Synth 01", "Machine Drum 01", "Synth 02"]
    assert app.panels[inst.id].winfo_ismapped()
    app.tabbar.menu.invoke(1)  # "Machine Drum" from the + menu: the panel is assembled in small steps
    for _ in range(300):
        app.root.update()
        if app.selected is not inst:
            break
        time.sleep(0.002)
    assert app.selected.name == "Machine Drum 02" and app.panels[app.selected.id].ready


def test_switching_tabs_keeps_panel_state_and_never_touches_the_engine(app):
    s2 = app.add_instrument("synth", defer=False)
    knob = app.panels[s2.id].widgets["filt_reso"]
    knob.set_value(0.61)       # what a drag does: move the knob...
    knob.on_change(0.61)       # ...and tell the instrument
    render(app)
    queues = lambda: [getattr(i, "_q", app.engine._q).qsize() + i._cq.qsize() for i in app.manager.instances] + [app.engine._q.qsize()]  # noqa: E731
    before = queues()
    app.select(app.engine.drums)
    app.select(next(i for i in app.manager.instances if i.name == "Synth 01"))
    app.select(s2)
    assert queues() == before
    assert app.panels[s2.id].widgets["filt_reso"].model.value == pytest.approx(0.61)
    assert s2.snapshot()["filt_reso"] == pytest.approx(0.61)


def test_close_selected_falls_back_to_neighbour(app):
    s2 = app.add_instrument("synth", defer=False)
    app.close_instance(s2)
    assert app.selected is app.manager.instances[-1] and s2.id not in app.panels
    assert app.engine.synth.name == "Synth 01"


def test_close_last_tab_allowed_and_plus_still_works(app):
    for inst in list(app.manager.instances):
        app.close_instance(inst)
    app.root.update()
    assert app.selected is None and labels(app) == []
    inst = app.add_instrument("synth", defer=False)
    assert app.selected is inst and app.engine.synth is inst


def test_midi_goes_to_selected_synth(app):
    s2 = app.add_instrument("synth", defer=False)
    app.engine.note_on(60, 1.0)
    render(app)
    assert s2.vm.active_notes() == [60] and app.manager.instances[0].vm.active_notes() == []


def test_held_notes_released_on_tab_switch(app):
    s1 = app.engine.synth
    released = []
    app.on_target_change = lambda: released.append(True)
    app.engine.note_on(60, 1.0)
    render(app)
    assert s1.vm.active_notes() == [60]
    app.add_instrument("synth", defer=False)
    render(app, 512)
    assert s1.vm.active_notes() == [] and released == [True]


def test_tab_badges_master_and_follower(app):
    app.manager.set_source(app.engine.drums, app.engine.synth)
    app.refresh_tabs()
    assert labels(app) == ["Synth 01 ●", "Machine Drum 01 ⇄ Synth 01"]


def test_header_fits_1366_with_many_tabs(app):
    for _ in range(4):
        app.add_instrument("synth", defer=False)
    app.root.update_idletasks()
    assert app.root.winfo_reqwidth() <= 1366 and app.root.winfo_reqheight() <= 768


def test_deferred_panel_build_selects_when_complete(app):
    inst = app.add_instrument("synth")          # deferred: assembled in small after() steps
    assert inst.id not in app.panels and app.selected is not inst
    for _ in range(200):
        app.root.update()
        if inst.id in app.panels:
            break
        time.sleep(0.002)
    assert inst.id in app.panels and app.selected is inst and app.panels[inst.id].ready


def test_graveyard_is_drained_by_the_app_poll(app):
    app.add_instrument("synth", defer=False)
    render(app)                                  # the audio thread adopts the topology and returns the old one
    assert not app.engine._grave.empty()
    app._poll()
    assert app.engine._grave.empty()


def test_closing_selected_tab_while_a_new_panel_builds_leaves_a_visible_editor(app):
    app.select(app.engine.drums)
    inst = app.add_instrument("synth")           # deferred build in progress
    app.close_instance(app.engine.drums)         # the fallback must not pick the unbuilt instance
    for _ in range(300):
        app.root.update()
        if inst.id in app.panels:
            break
        time.sleep(0.002)
    app.root.update()
    assert app.selected is inst and app.panels[inst.id].winfo_ismapped()
