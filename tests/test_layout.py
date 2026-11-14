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

    bindings: dict = {}

    def unbind(self, p):
        self.bindings = {k: v for k, v in self.bindings.items() if v != p}


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


def test_poll_survives_non_finite_apply_param():
    import pytest, tkinter
    from nomad.engine.engine import Engine
    try:
        a = build_app(Engine(48000))
    except tkinter.TclError:
        pytest.skip("no display")
    try:
        a.apply_param("polyphony", float("nan"))
        a.apply_param("filt_cutoff", float("inf"))
        a.apply_param("filt_reso", 0.5)
        a.root.update()
        a._poll()
        assert a._pending.empty() and a.widgets["filt_reso"].model.value == 0.5
    finally:
        a.close()


def test_bend_range_is_reachable():
    from nomad.gui.app import LAYOUT
    assert "bend_range" in LAYOUT["global"]


def test_midi_button_rescans_ports_for_hot_plug():
    import pytest, tkinter
    from nomad.engine.engine import Engine
    try:
        a = build_app(Engine(48000))
    except tkinter.TclError:
        pytest.skip("no display")
    try:
        ports, opened = [], []
        a.on_midi_port(opened.append, [], rescan=lambda: list(ports))
        assert str(a.midi_btn["state"]) == "normal"      # usable even with no ports yet
        a._cycle_midi(); assert opened == []              # nothing to open
        ports.extend(["USB Keyboard 20:0", "Midi Through 14:0"])
        a._cycle_midi(); assert opened == ["USB Keyboard 20:0"]  # keyboard plugged in later
        a._cycle_midi(); assert opened[-1] == "Midi Through 14:0"
    finally:
        a.close()


def _items(app, name):
    return {label: cb for label, cb in app._menu_items(name)}


def test_context_menu_resets_knobs_and_selectors_to_default(app):
    sent = []
    real = app.engine.set_param
    app.engine.set_param = lambda n, v: (sent.append((n, v)), real(n, v))
    app.widgets["filt_cutoff"].set_value(5000.0)
    app.widgets["osc1_wave"].set_index(3)
    _items(app, "filt_cutoff")["Valor por defecto"]()
    _items(app, "osc1_wave")["Valor por defecto"]()
    assert app.widgets["filt_cutoff"].model.value == pytest.approx(REGISTRY["filt_cutoff"].default)
    assert app.widgets["osc1_wave"].index == REGISTRY["osc1_wave"].default
    assert sent == [("filt_cutoff", REGISTRY["filt_cutoff"].default),
                    ("osc1_wave", REGISTRY["osc1_wave"].default)]
    p = REGISTRY["polyphony"]
    _items(app, "polyphony")["Valor por defecto"]()
    assert app.widgets["polyphony"].index == p.default - 1


def test_context_menu_learn_needs_a_map(app):
    assert _items(app, "filt_cutoff")["MIDI learn"] is None


def test_context_menu_learn_cancel_and_unbind():
    m = FakeMap()
    try:
        a = build_app(Engine(48000), midi_map=m)
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    try:
        _items(a, "osc1_wave")["MIDI learn"]()
        assert m.armed == ["osc1_wave"] and "osc1_wave" in a.status.get()
        assert "MIDI learn" not in _items(a, "osc1_wave")
        _items(a, "osc1_wave")["Cancelar learn"]()
        assert m.armed[-1] is None
        assert "Quitar MIDI" not in _items(a, "filt_reso")
        m.bindings = {("p", 0, 7): "filt_reso"}
        _items(a, "filt_reso")["Quitar MIDI"]()
        assert m.bindings == {} and "filt_reso" in a.status.get()
        a._context("filt_reso", None)  # no event: must not raise
    finally:
        a.close()
