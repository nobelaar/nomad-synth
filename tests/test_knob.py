import math
import tkinter as tk
from types import SimpleNamespace

import pytest

from nomad.engine.params import REGISTRY
from nomad.gui.knob import KnobModel


def test_drag_moves_norm_and_clamps():
    k = KnobModel(REGISTRY["filt_cutoff"], 1000.0)
    k.set_from_drag(1000, fine=False)
    assert k.norm == 1.0
    k.set_from_drag(-5000, fine=False)
    assert k.norm == 0.0


def test_fine_drag_is_slower():
    a = KnobModel(REGISTRY["filt_cutoff"], 1000.0); a.set_from_drag(100, fine=False)
    b = KnobModel(REGISTRY["filt_cutoff"], 1000.0); b.set_from_drag(100, fine=True)
    assert b.norm < a.norm


def test_value_respects_curve():
    k = KnobModel(REGISTRY["filt_cutoff"], 1000.0)
    k.norm = 0.5
    assert 100.0 < k.value < 2000.0   # exp: 0.5 no es la media aritmética


def test_reset_returns_default():
    k = KnobModel(REGISTRY["amp_sustain"], 0.1); k.reset()
    assert k.value == pytest.approx(REGISTRY["amp_sustain"].default)


def test_drag_200px_is_full_travel_and_fine_is_10x():
    k = KnobModel(REGISTRY["amp_sustain"], 0.0)
    k.set_from_drag(200, fine=False)
    assert k.norm == pytest.approx(1.0)
    k.norm = 0.0
    k.set_from_drag(200, fine=True)
    assert k.norm == pytest.approx(0.1)


def test_wheel_steps():
    k = KnobModel(REGISTRY["amp_sustain"], 0.5)
    k.set_from_wheel(1)
    assert k.norm == pytest.approx(0.52)
    k.set_from_wheel(-2, fine=True)
    assert k.norm == pytest.approx(0.516)
    k.set_from_wheel(-1000)
    assert k.norm == 0.0


def test_norm_clamps_and_rejects_nan():
    k = KnobModel(REGISTRY["amp_sustain"], 0.5)
    k.norm = 5
    assert k.norm == 1.0
    k.norm = -5
    assert k.norm == 0.0
    for bad in (math.nan, math.inf):
        with pytest.raises(ValueError):
            k.norm = bad
    with pytest.raises(ValueError):
        k.value = math.nan
    assert k.norm == 0.0


def test_value_roundtrip_and_clamp():
    s = REGISTRY["filt_cutoff"]
    k = KnobModel(s, 1000.0)
    assert k.value == pytest.approx(1000.0)
    k.value = 1e9
    assert k.value == pytest.approx(s.hi)
    k.value = -3
    assert k.value == pytest.approx(s.lo)


def test_format_value_units():
    f = lambda n, v: KnobModel(REGISTRY[n], v).format_value()
    assert f("filt_cutoff", 440.0) == "440 Hz"
    assert f("filt_cutoff", 12000.0) == "12.0 kHz"
    assert f("lfo1_rate", 0.5) == "0.50 Hz"
    assert f("amp_attack", 5.0) == "5.0 ms"
    assert f("amp_release", 2500.0) == "2.50 s"
    assert f("osc1_coarse", 7.0) == "+7 st"
    assert f("osc1_fine", -12.0) == "-12 ct"
    assert f("amp_sustain", 0.8) == "80%"
    assert f("filt_env_amt", 0.5) == "+50%"
    assert f("polyphony", 8.0) == "8 voices"


# ---- widgets (need a display) ----
@pytest.fixture
def root():
    try:
        r = tk.Tk()
    except tk.TclError:
        pytest.skip("no display")
    yield r
    r.destroy()


def ev(**kw):
    return SimpleNamespace(**{"x": 0, "y": 0, "state": 0, "num": 0, "delta": 0, **kw})


def test_knob_drag_fires_and_set_value_does_not(root):
    from nomad.gui.knob import Knob
    got = []
    m = KnobModel(REGISTRY["amp_sustain"], 0.5)
    k = Knob(root, m, got.append, label="SUS")
    k.pack(); root.update()
    assert k.find_all()
    k._press(ev(y=100)); k._drag(ev(y=0))      # 100 px up
    assert got == [pytest.approx(1.0)]
    k._drag(ev(y=-500))
    assert got[-1] == pytest.approx(1.0)
    k.set_value(0.2)
    assert len(got) == 1 and m.value == pytest.approx(0.2)
    k._wheel(ev(num=4)); assert got[-1] == pytest.approx(0.22)
    k._wheel(ev(delta=-120)); assert got[-1] == pytest.approx(0.2)
    k._reset(ev()); assert got[-1] == pytest.approx(0.8)


def test_knob_context_and_highlight(root):
    from nomad.gui.knob import Knob
    seen = []
    k = Knob(root, KnobModel(REGISTRY["amp_sustain"], 0.5), lambda v: None, on_context=seen.append)
    k.pack(); root.update()
    e = ev(); k._context(e)
    assert seen == [e]
    k.highlight(True); k.highlight(False)


def test_selector(root):
    from nomad.gui.selector import Selector
    got = []
    s = Selector(root, ["SIN", "SAW", "SQR"], 1, got.append)
    s.pack(); root.update()
    assert s.index == 1
    s.select(2); assert got == [2]
    s.select(99); assert s.index == 2 and got == [2, 2]
    s.select(-5); assert s.index == 0
    s.set_index(1); assert s.index == 1 and got[-1] == 0
