import tkinter as tk

import pytest

from nomad.gui.keyboard import Keyboard, note_at

W, H = 700, 100  # 2 octaves: white key = 50px, black key 30px wide, 60px tall

def test_white_keys():
    assert note_at(5, 90, W, H, 2, 48) == 48
    assert note_at(55, 90, W, H, 2, 48) == 50
    assert note_at(5, None, W, H, 2, 48) == 48
    assert note_at(695, 90, W, H, 2, 48) == 71

def test_black_wins_and_whites_below():
    assert note_at(50, 30, W, H, 2, 48) == 49      # C#3 straddles C|D
    assert note_at(40, 30, W, H, 2, 48) == 49      # left half of black key
    assert note_at(55, 80, W, H, 2, 48) == 50      # below the black key: white D
    assert note_at(149, 30, W, H, 2, 48) == 52     # E|F has no black key: white E
    assert note_at(150, 30, W, H, 2, 48) == 53     # F

def test_outside():
    assert note_at(-1, 5, W, H) is None and note_at(W, 5, W, H) is None and note_at(5, H, W, H) is None

@pytest.fixture
def tkroot():
    try:
        r = tk.Tk()
    except tk.TclError as e:
        pytest.skip(str(e))
    yield r
    r.destroy()

def test_press_glissando_release(tkroot):
    ev = []
    k = Keyboard(tkroot, lambda n, v: ev.append(("on", n, v)), lambda n: ev.append(("off", n)), 2, W, H, 48)
    E = lambda x, y=90: type("E", (), {"x": x, "y": y})
    k._press(E(5)); k._press(E(10)); k._press(E(55))
    assert ev == [("on", 48, 0.8), ("off", 48), ("on", 50, 0.8)]
    k._release(); k._release()
    assert ev[-1] == ("off", 50) and len(ev) == 4
    k.set_active([48]); k.set_active([48])
    assert tkroot.tk.call(k._w, "itemcget", k._keys[48], "-fill") == "#55e69a"
