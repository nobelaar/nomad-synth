import tkinter as tk

import pytest

from nomad.engine.engine import Engine
from nomad.engine.params import REGISTRY
from nomad.gui.app import HIDDEN, LAYOUT, build_app, widget_kind


def test_every_parameter_is_reachable():
    shown = {p for params in LAYOUT.values() for p in params}
    assert set(REGISTRY) - HIDDEN <= shown


def test_no_parameter_appears_twice():
    seen = [p for params in LAYOUT.values() for p in params]
    assert len(seen) == len(set(seen))


def test_layout_names_are_real_parameters():
    assert all(p in REGISTRY for p in {p for v in LAYOUT.values() for p in v})


def test_section_names():
    assert set(LAYOUT) == {"osc1", "osc2", "filter", "mixer", "amp_env", "filt_env",
                           "lfo1", "lfo2", "matrix", "global"}


def test_widget_kind_over_registry():
    for name in REGISTRY:
        discrete = name == "polyphony" or name.endswith(("_wave", "_mode", "_sync", "_src", "_dst"))
        assert widget_kind(name) == ("selector" if discrete else "knob"), name


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


class FakeMap:
    learning = None

    def __init__(self):
        self.armed = []

    def arm(self, p):
        self.armed.append(p)
        self.learning = p

    def save(self):
        pass


def test_window_fits_1366x768(app):
    app.root.update_idletasks()
    assert app.root.winfo_reqwidth() <= 1366 and app.root.winfo_reqheight() <= 768


def test_widgets_reflect_engine_and_drive_it(app):
    assert app.widgets["filt_cutoff"].model.value == pytest.approx(1200.0)
    assert app.widgets["polyphony"].index == 7
    sent = []
    app.engine.set_param = lambda n, v: sent.append((n, v))
    app.widgets["osc1_wave"].select(1)
    app.widgets["polyphony"].select(2)
    app.widgets["filt_reso"].on_change(0.5)
    assert sent == [("osc1_wave", 1), ("polyphony", 3), ("filt_reso", 0.5)]


def test_apply_param_updates_widget_and_engine(app):
    app.apply_param("filt_reso", 0.9)
    app.apply_param("mod3_src", 2)
    app.root.update()
    app._poll()
    assert app.widgets["filt_reso"].model.value == pytest.approx(0.9)
    assert app.widgets["mod3_src"].index == 2
    assert app.engine.snapshot()["filt_reso"] == pytest.approx(0.9)


def test_learn_arms_last_touched_and_requires_map(app):
    app.root.update()
    assert str(app.learn_btn["state"]) == "disabled"
    m = FakeMap()
    try:
        a2 = build_app(Engine(48000), midi_map=m)
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    try:
        a2.widgets["filt_cutoff"].on_change(2000.0)
        a2.learn_btn.invoke()
        assert m.armed == ["filt_cutoff"] and "filt_cutoff" in a2.status.get()
        a2.learn_btn.invoke()
        assert m.armed[-1] is None
    finally:
        a2.close()


def test_close_is_idempotent(app):
    app.close()
    app.close()
