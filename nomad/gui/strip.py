"""Per-instance strip: clock source, tempo, mute and level.

Intent lives here (``_muted``); the audio thread owns the real state and the
strip never reads it back to decide. Everything it changes goes through the
manager, which only enqueues commands or publishes topologies.
"""

from __future__ import annotations

import tkinter as tk

from nomad.gui.drums_view import _BTN
from nomad.gui.knob import BG, GREEN, GREY, TROUGH

_FONT = ("TkDefaultFont", 8)


def _scale(parent, lo, hi, length):
    return tk.Scale(parent, from_=lo, to=hi, orient="horizontal", length=length, showvalue=False, bg=BG, fg=GREY,
                    troughcolor=TROUGH, highlightthickness=0, bd=0, sliderlength=14, activebackground=GREEN,
                    takefocus=0)


class InstanceStrip(tk.Frame):
    def __init__(self, parent, app, inst, panel, with_bpm: bool, register_help=None):
        super().__init__(parent, bg=BG, highlightbackground=TROUGH, highlightthickness=1)
        self.app, self.inst, self.panel = app, inst, panel
        self._muted, self._was_following, self._syncing = False, False, False
        tk.Label(self, text="CLOCK / MIX", bg=BG, fg=GREEN, font=_FONT).pack(anchor="w", padx=4)
        self.clock_btn = tk.Button(self, text="CLOCK: Internal", command=self.cycle_source, **_BTN)
        self.clock_btn.pack(anchor="w", padx=2, pady=1)
        self.bpm = None
        if with_bpm:
            row = tk.Frame(self, bg=BG)
            row.pack(anchor="w", padx=2)
            tk.Label(row, text="BPM", bg=BG, fg=GREY, font=_FONT, width=5, anchor="w").pack(side="left")
            self._bpm_var = tk.DoubleVar(self, value=inst.clock.bpm)
            self._bpm_var.trace_add("write", lambda *_: self._on_bpm())
            self.bpm = _scale(row, 60, 180, 120)
            self.bpm.configure(variable=self._bpm_var)
            self.bpm.pack(side="left")
        row = tk.Frame(self, bg=BG)
        row.pack(anchor="w", padx=2, pady=1)
        self.mute_btn = tk.Button(row, text="MUTE", command=self.toggle_mute, **_BTN)
        self.mute_btn.pack(side="left", padx=(0, 6))
        self.level = _scale(row, 0, 100, 90)
        self._level_var = tk.DoubleVar(self, value=100)
        self._level_var.trace_add("write", lambda *_: app.manager.set_level(inst, self._level_var.get() / 100.0))
        self.level.configure(variable=self._level_var)
        self.level.pack(side="left")
        if register_help is not None:
            for w, key in ((self.clock_btn, "clock_source"), (self.mute_btn, "inst_mute"), (self.level, "inst_level")):
                register_help(w, key)
            if self.bpm is not None:
                register_help(self.bpm, "inst_bpm")

    # ---- user actions ------------------------------------------------------------
    def source_options(self):
        """Internal plus every other Internal instance (a master cannot follow; no chains)."""
        mgr, inst = self.app.manager, self.inst
        if mgr.followers(inst):
            return [None]
        return [None] + [i for i in mgr.instances if i is not inst and i.source is None]

    def cycle_source(self) -> None:
        opts = self.source_options()
        cur = self.inst.source
        nxt = opts[(opts.index(cur) + 1) % len(opts)] if cur in opts else None
        if not self.app.manager.set_source(self.inst, nxt):
            self.app.status.set("CLOCK: no se puede sincronizar con esa fuente")
        self.app.refresh_tabs()
        self.refresh()

    def toggle_mute(self) -> None:
        self._muted = not self._muted
        self.app.manager.set_mute(self.inst, self._muted)
        self.refresh()

    def _on_bpm(self) -> None:
        if not self._syncing:
            self.app.manager.set_bpm(self.inst, self._bpm_var.get())  # ignored for followers

    # ---- drawing ------------------------------------------------------------------
    def refresh(self) -> None:
        inst = self.inst
        following = inst.source is not None
        self.clock_btn.configure(text=f"CLOCK: {inst.source.name if following else 'Internal'}"[:26])
        self.mute_btn.configure(bg=GREEN if self._muted else TROUGH, fg=BG if self._muted else GREY)
        bpm = inst.eclock.bpm
        if self.bpm is not None:
            self._syncing = True
            try:
                if (following or self._was_following) and abs(self._bpm_var.get() - bpm) > 0.01:
                    self._bpm_var.set(bpm)  # follower shows the source tempo; going Internal keeps it
            finally:
                self._syncing = False
            self.bpm.configure(state="disabled" if following else "normal")
        elif following:
            self.panel.show_bpm(bpm)
        elif self._was_following:
            inst.values["bpm"] = bpm  # Internal again: the drum knob keeps the inherited tempo
        self.panel.set_transport_locked(following)
        self._was_following = following
