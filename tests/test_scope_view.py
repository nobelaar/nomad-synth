import tkinter as tk

import numpy as np
import pytest

from nomad.audio.scope import ScopeBuffer
from nomad.engine.engine import Engine
from nomad.gui.app import build_app
from nomad.gui.scope_view import ScopeView, find_trigger, to_points


def test_trigger_finds_rising_crossing():
    buf = np.sin(2*np.pi*np.arange(1000)/100.0)
    t = find_trigger(buf)
    assert buf[t-1] < 0 <= buf[t]
    assert t > 700   # el último cruce, no el primero

def test_trigger_on_flat_signal_returns_zero():
    assert find_trigger(np.zeros(100)) == 0

@pytest.mark.parametrize("buf", [[], [1.0], [-1.0]])
def test_trigger_short_buffers(buf):
    assert find_trigger(np.array(buf)) == 0

def test_trigger_with_nan_and_dc():
    b = np.sin(2*np.pi*np.arange(500)/100.0)
    b[250] = np.nan
    assert 0 <= find_trigger(b) < 500
    assert find_trigger(np.sin(2*np.pi*np.arange(500)/100.0) + 2) == 0

def test_trigger_near_end():
    b = np.concatenate([-np.ones(98), [1.0, 1.0]])
    assert find_trigger(b) == 98

def test_to_points_decimates_and_handles_empty():
    p = to_points(np.sin(np.arange(48000) / 10), 250, 100)
    assert len(p) <= 4 * 250 and all(0 <= y <= 100 for y in p[1::2])
    assert len(to_points(np.array([]), 250, 100)) == 4
    assert len(to_points(np.array([np.nan]), 250, 100)) == 4

@pytest.fixture
def tkroot():
    try:
        r = tk.Tk()
    except tk.TclError as e:
        pytest.skip(str(e))
    yield r
    r.destroy()

def test_scope_view_one_line_and_stop(tkroot):
    sb = ScopeBuffer(48000)
    v = ScopeView(tkroot, sb, 250, 150)
    v.redraw()  # empty buffer
    sb.write(np.sin(2*np.pi*440*np.arange(4800)/48000).astype(np.float32))
    n = len(v.canvas.find_all())
    v.redraw()
    assert len(v.canvas.find_all()) == n
    assert len(v.canvas.coords(v.line)) > 4
    v.set_time_ms(1e9)
    assert v.time_ms == 50
    with pytest.raises(ValueError):
        v.set_time_ms(float("nan"))
    v.stop()
    assert v._after is None

def test_panel_fits_with_scope_and_keyboard():
    try:
        a = build_app(Engine(48000))
    except tk.TclError as e:
        pytest.skip(str(e))
    try:
        a.root.update()
        assert a.root.winfo_reqwidth() <= 1366 and a.root.winfo_reqheight() <= 768
        assert a.scope.winfo_ismapped() and a.keyboard.winfo_ismapped()
    finally:
        a.close()
