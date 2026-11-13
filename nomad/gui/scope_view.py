"""Oscilloscope view: triggered, decimated, one canvas line updated in place."""

from __future__ import annotations

import math
import tkinter as tk

import numpy as np

from nomad.gui.knob import BG, GREEN, GREY, TROUGH

FRAME_MS = 33
SCALES_MS = (5, 10, 20, 50)


def find_trigger(buf: np.ndarray) -> int:
    """Index of the last rising zero crossing (buf[i-1] < 0 <= buf[i]); 0 if none.

    NaNs count as 0. A DC-offset signal that never crosses zero returns 0.
    """
    b = np.signbit(np.nan_to_num(np.asarray(buf, dtype=np.float32)))
    idx = np.flatnonzero(b[:-1] & ~b[1:])
    return int(idx[-1]) + 1 if len(idx) else 0


def to_points(seg: np.ndarray, width: int, height: int) -> list[float]:
    """Flat [x0, y0, x1, y1, ...] canvas coords, at most 2*width points (min/max decimation)."""
    seg = np.nan_to_num(np.asarray(seg, dtype=np.float32))
    m = len(seg)
    if m == 0:
        return [0.0, height / 2, float(width), height / 2]
    if m >= 2 * width:
        k = m // width
        cols = seg[: k * width].reshape(width, k)
        seg = np.stack((cols.min(1), cols.max(1)), axis=1).ravel()
    ys = height / 2 - np.clip(seg, -1, 1) * (height / 2 - 2)
    xs = np.linspace(0, width, len(ys)) if len(ys) > 1 else np.zeros(1)
    out = np.empty(2 * len(ys))
    out[0::2], out[1::2] = xs, ys
    if len(ys) == 1:
        out = np.array([0.0, ys[0], float(width), ys[0]])
    return out.tolist()


class ScopeView(tk.Frame):
    """Canvas scope plus 5/10/20/50 ms buttons.

    Trigger: read the last 2*n samples (n = displayed window); search the trigger
    only in the FIRST n of them and draw n samples from it, so a full window always
    follows the trigger. With a not-yet-full buffer it draws from the start.
    """

    def __init__(self, parent, buffer, width=250, height=150, sr=48000):
        super().__init__(parent, bg=BG, width=width, height=height)
        self.buffer, self.sr, self.w, self.h = buffer, sr, width, height - 24
        self.time_ms = 10.0
        self._after = None
        self.canvas = tk.Canvas(self, width=self.w, height=self.h, bg=BG, highlightthickness=0)
        self.canvas.pack()
        self.canvas.create_line(0, self.h / 2, self.w, self.h / 2, fill=TROUGH)  # center + 3 divisions
        for i in range(1, 4):
            self.canvas.create_line(i * self.w / 4, 0, i * self.w / 4, self.h, fill=TROUGH)
        self.line = self.canvas.create_line(0, self.h / 2, self.w, self.h / 2, fill=GREEN)
        bar = tk.Frame(self, bg=BG)
        bar.pack(anchor="w")
        self.buttons = {}
        for ms in SCALES_MS:
            b = tk.Button(bar, text=f"{ms}ms", command=lambda m=ms: self.set_time_ms(m), bg=TROUGH, fg=GREY,
                          activebackground=GREEN, activeforeground=BG, relief="flat", bd=0, padx=4, pady=1,
                          font=("TkDefaultFont", 8), takefocus=0)
            b.pack(side="left", padx=1, pady=2)
            self.buttons[ms] = b
        self.set_time_ms(self.time_ms)
        self.bind("<Destroy>", lambda e: self.stop() if e.widget is self else None)
        self._after = self.after(FRAME_MS, self._tick)

    def set_time_ms(self, ms) -> None:
        ms = float(ms)
        if not math.isfinite(ms):
            raise ValueError(f"time must be finite, got {ms!r}")
        self.time_ms = min(max(ms, 1.0), float(SCALES_MS[-1]))
        for m, b in self.buttons.items():
            b.configure(fg=BG if m == self.time_ms else GREY, bg=GREEN if m == self.time_ms else TROUGH)

    def redraw(self) -> None:
        n = max(2, int(self.time_ms * self.sr / 1000))
        w = self.buffer.window(2 * n)
        region = w[: len(w) - n] if len(w) > n else w[:0]
        t = find_trigger(region)
        pts = to_points(w[t:t + n], self.w, self.h)
        try:
            self.canvas.coords(self.line, *pts)
        except tk.TclError:
            pass  # window already destroyed

    def _tick(self):
        self._after = None
        self.redraw()
        try:
            self._after = self.after(FRAME_MS, self._tick)
        except tk.TclError:
            pass

    def stop(self) -> None:
        """Cancel the redraw loop (idempotent)."""
        if self._after is not None:
            try:
                self.after_cancel(self._after)
            except tk.TclError:
                pass
            self._after = None
