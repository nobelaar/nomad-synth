"""Drum machine view: transport, per-instrument knobs and a 16-step grid.

Only the Tk thread touches widgets. The view reads ``DrumMachine`` state
(``steps``, ``values``, ``position``, ``playing``) and writes through its
thread-safe setters; ``refresh`` runs from the panel's 100 ms poll.
"""

from __future__ import annotations

import tkinter as tk

from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS, STEPS, DrumMachine
from nomad.gui.knob import BG, GREEN, GREY, TROUGH, Knob, KnobModel

ACCENT = "#b9ffd8"
PLAYHEAD = "#ffffff"
_FONT = ("TkDefaultFont", 8)
_BTN = dict(bg=TROUGH, fg=GREY, activebackground=GREEN, activeforeground=BG,
            relief="flat", bd=0, padx=6, pady=2, font=_FONT, takefocus=0,
            disabledforeground="#555555")
_SHORT = {"tune": "tune", "decay": "decay", "level": "level", "bpm": "BPM", "swing": "SWING"}


class DrumView(tk.Frame):
    def __init__(self, parent, drums: DrumMachine, register_help=None, on_context=None):
        super().__init__(parent, bg=BG)
        self.drums = drums
        self.knobs: dict[str, Knob] = {}
        self._steps: list[list[tk.Button]] = []
        self._help = register_help
        self._context = on_context
        self._build()

    # ---- construction ------------------------------------------------------
    def _build(self):
        top = tk.Frame(self, bg=BG)
        top.pack(anchor="w", padx=2, pady=2)
        self.play_btn = tk.Button(top, text="PLAY", command=self.toggle_transport, **_BTN)
        self.play_btn.pack(side="left", padx=(0, 8))
        self._knob(top, "bpm").pack(side="left", padx=2)
        self._knob(top, "swing").pack(side="left", padx=2)
        self.clear_btn = tk.Button(top, text="CLEAR", command=self.clear, **_BTN)
        self.clear_btn.pack(side="left", padx=8)
        self._register(self, "drums_view")
        self._register(top, "drum_transport")

        for track in INSTRUMENTS:
            row = tk.Frame(self, bg=BG)
            row.pack(anchor="w", pady=1)
            name = tk.Button(row, text=track.upper(), width=6, **_BTN,
                             command=lambda t=track: self.drums.trigger(t))
            name.pack(side="left", padx=(2, 4))
            self._register(name, f"drum_{track}")
            for field in ("tune", "decay", "level"):
                self._knob(row, f"{track}_{field}").pack(side="left", padx=1)

            grid = tk.Frame(row, bg=BG)
            grid.pack(side="left", padx=6)
            row_steps = []
            for step in range(STEPS):
                b = tk.Button(grid, text="", width=2, **_BTN)
                b.pack(side="left", padx=1)
                b.bind("<Button-1>", lambda e, t=track, s=step: self.toggle_step(t, s, bool(e.state & 1)))
                self._register(b, "drum_steps")
                row_steps.append(b)
            self._steps.append(row_steps)
            self._paint_row(len(self._steps) - 1)
        self.refresh()

    def _knob(self, parent, name: str) -> Knob:
        spec = DRUM_REGISTRY[name]
        knob = Knob(parent, KnobModel(spec, self.drums.values[name]),
                    lambda v, n=name: self.set_value(n, v), label=_SHORT[name.rsplit("_", 1)[-1]],
                    on_context=lambda e, n=name: self._context(n, e) if self._context else None)
        self.knobs[name] = knob
        self._register(knob, name)
        return knob

    def _register(self, widget, key: str) -> None:
        if self._help is not None:
            self._help(widget, key)

    # ---- user actions ------------------------------------------------------
    def toggle_step(self, track: str, step: int, accent: bool = False) -> None:
        i = INSTRUMENTS.index(track)
        current = int(self.drums.steps[i, step])
        if accent:
            new = 0 if current == 2 else 2
        else:
            new = 0 if current else 1
        self.drums.set_step(track, step, new)
        self._paint(i, step)

    def set_value(self, name: str, value: float) -> None:
        self.drums.set_value(name, value)
        knob = self.knobs.get(name)
        if knob is not None:
            knob.set_value(self.drums.values[name])

    def reset(self, name: str) -> None:
        self.set_value(name, DRUM_REGISTRY[name].default)

    def toggle_transport(self) -> None:
        self.drums.toggle()
        self.refresh()

    def clear(self) -> None:
        self.drums.clear_pattern()
        for i in range(len(INSTRUMENTS)):
            self._paint_row(i)

    # ---- drawing -----------------------------------------------------------
    def _paint(self, track: int, step: int, current: bool = False) -> None:
        level = int(self.drums.steps[track, step])
        b = self._steps[track][step]
        b.configure(bg=GREEN if level == 1 else ACCENT if level == 2 else TROUGH,
                    fg=BG if level else GREY,
                    highlightthickness=2, highlightbackground=PLAYHEAD if current else BG)

    def _paint_row(self, track: int) -> None:
        for step in range(STEPS):
            self._paint(track, step)

    def refresh(self) -> None:
        self.drums.flush()
        for name, knob in self.knobs.items():
            value = self.drums.values[name]
            if abs(knob.model.value - value) > 1e-9:
                knob.set_value(value)
        position = self.drums.position if self.drums.playing else -1
        for track in range(len(INSTRUMENTS)):
            for step in range(STEPS):
                self._paint(track, step, current=(step == position))
        playing = self.drums.playing
        self.play_btn.configure(text="STOP" if playing else "PLAY",
                                bg=GREEN if playing else TROUGH, fg=BG if playing else GREY)
