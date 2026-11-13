"""On-screen piano keyboard. Talks to engine.note_on/note_off directly (never MidiRouter)."""

from __future__ import annotations

import tkinter as tk

from nomad.gui.knob import BG, GREEN, TROUGH

WHITE_PC = (0, 2, 4, 5, 7, 9, 11)
BLACK_AFTER = {0: 1, 1: 3, 3: 6, 4: 8, 5: 10}  # white index in octave -> black pitch class
BLACK_W, BLACK_H = 0.6, 0.6                     # fractions of white width / keyboard height
VELOCITY = 0.8
BASE_NOTE = 36  # C2 default; a wide (6-octave) panel keyboard then spans C2..B7


def note_at(x, y, width, height, octaves=2, base=BASE_NOTE) -> int | None:
    """MIDI note under (x, y) or None. Black keys win where they overlap whites.

    ``y=None`` means 'low on the key' (white). Pure: no Tk needed.
    """
    if not (0 <= x < width) or (y is not None and not (0 <= y < height)):
        return None
    ww = width / (7 * octaves)
    wi = int(x // ww)
    if y is not None and y < BLACK_H * height:
        for j in (wi, wi + 1):  # black key straddling the boundary left/right of x
            if (j - 1) % 7 in BLACK_AFTER and 1 <= j < 7 * octaves and abs(x - j * ww) < BLACK_W * ww / 2:
                return base + 12 * ((j - 1) // 7) + BLACK_AFTER[(j - 1) % 7]
    return base + 12 * (wi // 7) + WHITE_PC[wi % 7]


class Keyboard(tk.Canvas):
    def __init__(self, parent, on_note_on, on_note_off, octaves=2, width=1350, height=84, base=BASE_NOTE):
        super().__init__(parent, width=width, height=height, bg=BG, highlightthickness=0)
        self.on_note_on, self.on_note_off = on_note_on, on_note_off
        self.octaves, self.base, self.w, self.h = octaves, base, width, height
        self._held: int | None = None
        self._active: set[int] = set()
        self._keys: dict[int, int] = {}
        ww = width / (7 * octaves)
        for i in range(7 * octaves):  # whites first so blacks draw on top
            n = base + 12 * (i // 7) + WHITE_PC[i % 7]
            self._keys[n] = self.create_rectangle(i * ww, 0, (i + 1) * ww, height, fill="#d8d8d8", outline=TROUGH)
        for j in range(1, 7 * octaves):
            if (j - 1) % 7 in BLACK_AFTER:
                n = base + 12 * ((j - 1) // 7) + BLACK_AFTER[(j - 1) % 7]
                self._keys[n] = self.create_rectangle(j * ww - BLACK_W * ww / 2, 0, j * ww + BLACK_W * ww / 2,
                                                      BLACK_H * height, fill="#1c1c1c", outline=TROUGH)
        self.bind("<ButtonPress-1>", self._press)
        self.bind("<B1-Motion>", self._press)
        for ev in ("<ButtonRelease-1>", "<Leave>", "<FocusOut>"):
            self.bind(ev, self._release)
        self.bind("<Destroy>", lambda e: self._release() if e.widget is self else None)
        self.winfo_toplevel().bind("<FocusOut>", self._release, add="+")

    def note_at(self, x, y=None):
        return note_at(x, y, self.w, self.h, self.octaves, self.base)

    def _press(self, e):
        n = self.note_at(e.x, e.y)
        if n == self._held:
            return
        self._release()
        if n is not None:
            self._held = n
            self.on_note_on(n, VELOCITY)
            self._paint()

    def _release(self, _e=None):
        if self._held is not None:
            n, self._held = self._held, None
            self.on_note_off(n)
            self._paint()

    def set_active(self, notes) -> None:
        """Light keys for notes sounding elsewhere (e.g. MIDI input)."""
        notes = set(notes)
        if notes != self._active:
            self._active = notes
            self._paint()

    def _paint(self):
        lit = self._active | ({self._held} if self._held is not None else set())
        try:
            for n, item in self._keys.items():
                black = (n - self.base) % 12 in (1, 3, 6, 8, 10)
                self.itemconfigure(item, fill=GREEN if n in lit else ("#1c1c1c" if black else "#d8d8d8"))
        except tk.TclError:
            pass  # widget already gone (<Destroy> while a key is held)
