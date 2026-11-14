"""Rotary knob: pure-logic KnobModel plus a Canvas widget (importing tkinter needs no display)."""

from __future__ import annotations

import math
import tkinter as tk

from nomad.engine.params import ParamSpec

BG, GREEN, GREY, TROUGH = "#101310", "#55e69a", "#999999", "#29342b"
DRAG_PX = 200          # pixels of vertical drag for the full 0..1 travel
FINE = 10.0            # shift divides the drag/wheel step by this
WHEEL_STEP = 0.02      # norm per wheel notch (50 notches = full travel)


class KnobModel:
    """Value <-> norm (0..1, through the spec's curve). Never calls back."""

    def __init__(self, spec: ParamSpec, value: float):
        self.spec = spec
        self._norm = 0.0
        self.value = value

    @property
    def norm(self) -> float:
        return self._norm

    @norm.setter
    def norm(self, n: float) -> None:
        n = float(n)
        if not math.isfinite(n):
            raise ValueError(f"norm must be finite, got {n!r}")
        self._norm = min(max(n, 0.0), 1.0)

    @property
    def value(self) -> float:
        s = self.spec
        if s.curve == "exp":
            return s.lo * (s.hi / s.lo) ** self._norm
        return s.lo + (s.hi - s.lo) * self._norm

    @value.setter
    def value(self, v: float) -> None:
        v = float(v)
        if not math.isfinite(v):
            raise ValueError(f"value must be finite, got {v!r}")
        s = self.spec
        v = min(max(v, s.lo), s.hi)
        if s.curve == "exp":
            self.norm = math.log(v / s.lo) / math.log(s.hi / s.lo)
        else:
            self.norm = (v - s.lo) / (s.hi - s.lo)

    def set_from_drag(self, pixels: int, fine: bool = False) -> None:
        """Incremental: positive pixels (drag up) increase the value."""
        self.norm = self._norm + pixels / DRAG_PX / (FINE if fine else 1.0)

    def set_from_wheel(self, delta: int, fine: bool = False) -> None:
        """delta = notches (+ up). Step 0.02 of travel, 0.002 with fine."""
        self.norm = self._norm + delta * WHEEL_STEP / (FINE if fine else 1.0)

    def reset(self) -> None:
        self.value = self.spec.default

    def format_value(self) -> str:
        s, v, u = self.spec, self.value, self.spec.unit
        if u == "ms":
            if v >= 1000:
                return f"{v / 1000:.2f} s"
            return f"{v:.0f} ms" if v >= 100 else f"{v:.1f} ms"
        if u == "Hz":
            if v >= 10000:
                return f"{v / 1000:.1f} kHz"
            if v >= 1000:
                return f"{v / 1000:.2f} kHz"
            return f"{v:.0f} Hz" if v >= 100 else f"{v:.1f} Hz" if v >= 10 else f"{v:.2f} Hz"
        if u in ("st", "ct"):
            return f"{v:+.0f} {u}"
        if u == "voices":
            return f"{v:.0f} voices"
        if u:
            return f"{v:g} {u}"
        pct = round(v * 100)
        return f"{pct:+d}%" if s.lo < 0 else f"{pct}%"


class Knob(tk.Canvas):
    """Canvas knob: drag up/down (shift = fine), wheel, double-click = default.

    on_change(value) fires on user gestures only; set_value() is silent.
    on_context(event) is bound to right and middle click (MIDI learn).
    """

    W, H, R = 64, 84, 21
    START, SWEEP = 225, -270  # tk arc degrees: from lower-left, clockwise

    def __init__(self, parent, model, on_change, label=None, on_context=None):
        super().__init__(parent, width=self.W, height=self.H, bg=BG,
                         highlightthickness=0, cursor="sb_v_double_arrow")
        self.model, self.on_change, self.on_context = model, on_change, on_context
        self.label = label if label is not None else model.spec.name
        self._y = 0
        self._hl = False
        self.locked = False  # a locked knob shows its value but ignores the user (e.g. BPM of a follower)
        self.bind("<ButtonPress-1>", self._press)
        self.bind("<B1-Motion>", self._drag)
        self.bind("<Double-Button-1>", self._reset)
        self.bind("<Button-4>", self._wheel)
        self.bind("<Button-5>", self._wheel)
        self.bind("<MouseWheel>", self._wheel)
        for b in ("<Button-2>", "<Button-3>"):
            self.bind(b, self._context)
        self._draw()

    def _draw(self):
        self.delete("all")
        cx, cy, r = self.W / 2, 14 + self.R + 4, self.R
        box = (cx - r, cy - r, cx + r, cy + r)
        self.create_text(cx, 8, text=self.label, fill=GREEN if self._hl else GREY,
                         font=("TkDefaultFont", 8))
        self.create_arc(*box, start=self.START, extent=self.SWEEP, style="arc",
                        outline=TROUGH, width=5)
        n = self.model.norm
        if n > 0:
            self.create_arc(*box, start=self.START, extent=self.SWEEP * n,
                            style="arc", outline=GREEN, width=5)
        a = math.radians(self.START + self.SWEEP * n)
        self.create_line(cx, cy, cx + (r - 4) * math.cos(a), cy - (r - 4) * math.sin(a),
                         fill=GREEN, width=2)
        self.create_text(cx, self.H - 9, text=self.model.format_value(),
                         fill=GREEN if self._hl else GREY, font=("TkDefaultFont", 8))

    def _fire(self, before):
        if self.model.norm != before:
            self._draw()
            self.on_change(self.model.value)

    def _press(self, e):
        self._y = e.y

    def _drag(self, e):
        if self.locked:
            return
        before = self.model.norm
        self.model.set_from_drag(self._y - e.y, bool(e.state & 1))
        self._y = e.y
        self._fire(before)

    def _wheel(self, e):
        if self.locked:
            return
        d = 1 if (e.num == 4 or e.delta > 0) else -1
        before = self.model.norm
        self.model.set_from_wheel(d, bool(e.state & 1))
        self._fire(before)

    def _reset(self, e):
        if self.locked:
            return
        before = self.model.norm
        self.model.reset()
        self._draw()
        self.on_change(self.model.value)  # always fire: default may equal current

    def _context(self, e):
        if self.on_context:
            self.on_context(e)

    def set_value(self, v):
        self.model.value = v
        self._draw()

    def highlight(self, on: bool):
        if on != self._hl:
            self._hl = on
            self._draw()
