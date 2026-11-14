"""One Synth instance's control panel: parameter sections, sequencer editor and strip.

Each panel owns its own widgets and talks only to its instrument, so several
Synth tabs can coexist. ``app`` provides the shared services (help registry,
right-click / MIDI-learn menu, status line).
"""

from __future__ import annotations

import tkinter as tk

from nomad.engine.params import REGISTRY
from nomad.gui.knob import BG, GREEN, GREY, TROUGH, Knob, KnobModel
from nomad.gui.selector import Selector
from nomad.gui.seq_view import SeqView

_P = [f"mod{i}_{k}" for i in range(8) for k in ("src", "dst", "amt")]
LAYOUT: dict[str, list[str]] = {
    "osc1": ["osc1_wave", "osc1_mode", "osc1_coarse", "osc1_hz", "osc1_fine", "osc1_pw"],
    "osc2": ["osc2_wave", "osc2_mode", "osc2_sync", "osc2_coarse", "osc2_hz", "osc2_fine", "osc2_pw"],
    "mixer": ["osc1_level", "osc2_level"],
    "filter": ["filt_cutoff", "filt_reso", "filt_env_amt"],
    "amp_env": ["amp_attack", "amp_decay", "amp_sustain", "amp_release"],
    "filt_env": ["filt_attack", "filt_decay", "filt_sustain", "filt_release"],
    "lfo1": ["lfo1_wave", "lfo1_rate", "lfo1_depth", "lfo1_phase"],
    "lfo2": ["lfo2_wave", "lfo2_rate", "lfo2_depth", "lfo2_phase"],
    "matrix": _P,
    "global": ["master_volume", "polyphony", "bend_range"],
}
HIDDEN = {"osc1_sync"}  # osc1_sync: engine only reads osc2_sync (dead param)

_SEL_SUFFIX = ("_wave", "_mode", "_sync", "_src", "_dst")
_LABELS = {  # abbreviations keep the 8-button destination row narrow
    "_mode": ["KEY", "FREE"], "_sync": ["OFF", "SYNC"],
    "_src": ["-", "LFO1", "LFO2", "MW", "VEL", "ENV2"],
    "_dst": ["-", "PIT", "P1", "P2", "AMP", "CUT", "PW1", "PW2"],
}
_FONT = ("TkDefaultFont", 8)


def widget_kind(name: str) -> str:
    """'selector' for discrete params (must never be a knob), else 'knob'."""
    return "selector" if name == "polyphony" or name.endswith(_SEL_SUFFIX) else "knob"


def _labels(name: str) -> list[str]:
    if name == "polyphony":
        return [str(i) for i in range(1, 9)]
    if name.endswith("_wave"):
        return ["SIN", "TRI", "SAW", "SQR"] + (["S&H"] if name.startswith("lfo") else [])
    return _LABELS[name[name.rindex("_"):]]


def _short(name: str) -> str:
    if name == "master_volume":
        return "volume"
    if name.endswith("_level"):
        return name[:4]  # mixer: osc1 / osc2
    if name.startswith("mod"):
        return f"amt {int(name[3]) + 1}"
    return name.split("_", 1)[1] if name.split("_")[0] in ("osc1", "osc2", "lfo1", "lfo2", "amp", "filt") else name


def _text(parent, **kw):
    return tk.Label(parent, bg=BG, font=_FONT, **kw)


def make_control(parent, name, user, context, help_register):
    """Knob or Selector for one parameter; ``user(name, value)`` is called on every user change."""
    spec = REGISTRY[name]
    if widget_kind(name) == "selector":
        off = 1 if name == "polyphony" else 0
        w = Selector(parent, _labels(name), 0, lambda i, n=name, o=off: user(n, i + o),
                     on_context=lambda e, n=name: context(n, e))
    else:
        w = Knob(parent, KnobModel(spec, spec.default), lambda v, n=name: user(n, v),
                 label=_short(name), on_context=lambda e, n=name: context(n, e))
    help_register(w, name)
    return w


class SynthPanel(tk.Frame):
    def __init__(self, parent, app, instrument, side=None, eager=True):
        """``side``: a frame in the right column that hosts this instance's VOICE section."""
        super().__init__(parent, bg=BG)
        self.app, self.instrument = app, instrument
        self.widgets: dict[str, Knob | Selector] = {}
        self.seq_view = None
        self.side_frame = tk.Frame(side if side is not None else self, bg=BG)
        self.side_frame.grid(row=0, column=0, sticky="nw")
        self.visible = True
        # Built one step at a time: Tk widget construction can hold the GIL, so a panel created while the
        # audio runs is assembled over several ``after`` ticks instead of one long stall.
        self._steps = [lambda s=sec, r=r, c=c: self.section(s).grid(row=r, column=c, sticky="nw", padx=2, pady=2)
                       for sec, r, c in (("osc1", 0, 0), ("osc2", 0, 1), ("filter", 0, 2), ("mixer", 0, 3),
                                         ("amp_env", 1, 0), ("filt_env", 1, 1), ("lfo1", 1, 2), ("lfo2", 1, 3))]
        self._steps += [self._build_matrix, self._build_seq, self._build_voice]
        if eager:
            self.build_all()

    @property
    def ready(self) -> bool:
        return not self._steps

    def build_next(self) -> bool:
        """Build one section; True while more remain."""
        if self._steps:
            self._steps.pop(0)()
        return bool(self._steps)

    def build_all(self) -> None:
        while self.build_next():
            pass

    def _build_matrix(self):
        self.section("matrix").grid(row=2, column=0, columnspan=4, sticky="nw", padx=2, pady=2)

    def _build_seq(self):
        self.seq_view = SeqView(self, self.instrument, register_help=self.app._help_register)
        self.seq_view.grid(row=3, column=0, columnspan=4, sticky="nw", padx=2, pady=2)

    def _build_voice(self):
        # polyphony / bend_range are per-instance; master_volume stays app-level (the mix volume)
        self.voice = self.section("global", [n for n in LAYOUT["global"] if n != "master_volume"], "VOICE",
                                  parent=self.side_frame)
        self.voice.pack(anchor="nw")

    def set_visible(self, on: bool) -> None:
        self.visible = on
        for f in (self, self.side_frame):
            f.grid() if on else f.grid_remove()

    def user(self, name, value):
        self.app._last = name
        self.instrument.set_param(name, value)

    def make(self, parent, name):
        w = make_control(parent, name, self.user, self.app._context, self.app._help_register)
        self.widgets[name] = w
        return w

    def section(self, sec, names=None, title=None, parent=None):
        f = tk.Frame(parent if parent is not None else self, bg=BG, highlightbackground=TROUGH, highlightthickness=1)
        _text(f, text=title or sec.upper().replace("_", " "), fg=GREEN).pack(anchor="w", padx=4)
        if sec == "matrix":
            grid = tk.Frame(f, bg=BG)
            grid.pack(anchor="w")
            for i in range(8):
                slot = tk.Frame(grid, bg=BG)
                slot.grid(row=i // 3, column=i % 3, padx=4, pady=1, sticky="w")
                sels = tk.Frame(slot, bg=BG)
                sels.pack(side="left")
                for k in ("src", "dst"):
                    self.make(sels, f"mod{i}_{k}").pack(anchor="w", pady=1)
                self.make(slot, f"mod{i}_amt").pack(side="left")
            return f
        top, row = tk.Frame(f, bg=BG), tk.Frame(f, bg=BG)
        top.pack(anchor="w", padx=2)
        row.pack(anchor="w")
        for n in names if names is not None else LAYOUT[sec]:
            self.make(top if widget_kind(n) == "selector" else row, n).pack(side="left", padx=2)
        return f

    # ---- instrument <-> widgets ------------------------------------------------
    def show(self, name, value):
        w = self.widgets.get(name)
        if isinstance(w, Selector):
            w.set_index(round(value) - (1 if name == "polyphony" else 0))
        elif w is not None:
            w.set_value(value)

    def push_snapshot(self):
        snap = self.instrument.snapshot()
        for n in self.widgets:
            self.show(n, snap[n])

    def refresh(self):
        if self.seq_view is not None:
            self.seq_view.refresh()

    def set_transport_locked(self, locked: bool) -> None:
        if self.seq_view is not None:
            self.seq_view.set_transport_locked(locked)
