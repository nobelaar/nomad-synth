"""NOMAD instrument panel (1366x768, no dropdowns).

Ownership: App owns only its Tk window and polls. It does NOT own the engine,
stream, MIDI input or MIDI map; whoever built them (Task 18) stops/closes them.
``App.scope`` (ScopeView) and ``App.keyboard`` (Keyboard, talks to the engine directly) live in
``scope_frame`` / ``keyboard_frame``.

Threading: only the Tk thread may touch widgets. ``apply_param`` is safe from
any thread (e.g. rtmidi's): it calls engine.set_param (a lock-free queue) and
queues the widget refresh, which the 100 ms poll applies on the Tk thread.
"""

from __future__ import annotations

import queue
import tkinter as tk

from nomad.audio.scope import ScopeBuffer
from nomad.audio.stream import list_output_devices
from nomad.engine.modmatrix import DESTINATIONS, SOURCES
from nomad.engine.params import REGISTRY
from nomad.gui.knob import BG, GREEN, GREY, TROUGH, Knob, KnobModel
from nomad.gui.keyboard import Keyboard
from nomad.gui.scope_view import ScopeView
from nomad.gui.selector import Selector

POLL_MS = 100
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


class App:
    def __init__(self, engine, stream=None, midi_map=None):
        self.engine, self.stream, self.midi_map = engine, stream, midi_map
        self.widgets: dict[str, Knob | Selector] = {}
        self.scope_buffer = (getattr(stream, "scope", None) if stream else None) or ScopeBuffer(engine.sr)
        self.root = root = tk.Tk()
        root.title("NOMAD SYNTH")
        root.configure(bg=BG)
        root.geometry("1366x768")
        root.resizable(False, False)
        self._after = None
        self._closed = False
        self._pending: queue.SimpleQueue = queue.SimpleQueue()
        self._last: str | None = None
        self._armed_by_us: str | None = None
        self._midi_cb = None
        self._midi_ports: list[str] = []
        self._midi_i = 0
        self.status = tk.StringVar(value="")
        self._build()
        self._push_snapshot()
        self._after = root.after(POLL_MS, self._poll)

    # ---- construction ------------------------------------------------------
    def _build(self):
        root = self.root
        self._build_header(tk.Frame(root, bg=BG)).grid(row=0, column=0, columnspan=2, sticky="ew", padx=6, pady=4)
        left = tk.Frame(root, bg=BG)
        left.grid(row=1, column=0, sticky="nw", padx=6)
        right = tk.Frame(root, bg=BG)
        right.grid(row=1, column=1, sticky="nw", padx=6)
        for sec, r, c in (("osc1", 0, 0), ("osc2", 0, 1), ("filter", 0, 2), ("mixer", 0, 3),
                          ("amp_env", 1, 0), ("filt_env", 1, 1), ("lfo1", 1, 2), ("lfo2", 1, 3)):
            self._section(left, sec).grid(row=r, column=c, sticky="nw", padx=2, pady=2)
        self._section(left, "matrix").grid(row=2, column=0, columnspan=4, sticky="nw", padx=2, pady=2)
        self.scope_frame = tk.Frame(right, bg=BG, width=250, height=150)
        self.scope_frame.pack_propagate(False)
        self.scope_frame.pack(anchor="nw", pady=2)
        self.scope = ScopeView(self.scope_frame, self.scope_buffer, 250, 150, sr=self.engine.sr)
        self.scope.pack()
        self._section(right, "global").pack(anchor="nw", pady=2)
        self.keyboard_frame = tk.Frame(root, bg=BG, width=1350, height=84)
        self.keyboard_frame.pack_propagate(False)
        self.keyboard_frame.grid(row=2, column=0, columnspan=2, padx=6, pady=4)
        self.keyboard = Keyboard(self.keyboard_frame, self.engine.note_on, self.engine.note_off, octaves=6)
        self.keyboard.pack()

    def _build_header(self, h):
        tk.Label(h, text="NOMAD SYNTH", bg=BG, fg=GREEN, font=("TkDefaultFont", 14, "bold")).pack(side="left", padx=8)
        devs = list_output_devices()
        self._devs = devs
        cur = getattr(self.stream, "device_index", None)
        self._dev_i = next((i for i, d in enumerate(devs) if d.index == cur), None)  # None: auto/unknown
        self.dev_btn = self._btn(h, self._cycle_device)
        self._set_dev_text()
        self.dev_btn.pack(side="left", padx=6)
        self.midi_btn = self._btn(h, self._cycle_midi)
        self._set_midi_text()
        self.midi_btn.pack(side="left", padx=6)
        self.voices = tk.StringVar(value="VOICES 0")
        self.xruns = tk.StringVar(value="XRUNS 0")
        for v in (self.voices, self.xruns):
            tk.Label(h, textvariable=v, bg=BG, fg=GREY, font=_FONT, width=10).pack(side="left", padx=6)
        self.learn_btn = self._btn(h, self._toggle_learn, "LEARN")
        if self.midi_map is None:
            self.learn_btn.configure(state="disabled")
            self.status.set("LEARN unavailable: no MIDI map")
        self.learn_btn.pack(side="left", padx=6)
        tk.Label(h, textvariable=self.status, bg=BG, fg=GREEN, font=_FONT).pack(side="left", padx=6)
        return h

    @staticmethod
    def _btn(parent, cmd, text=""):
        return tk.Button(parent, text=text, command=cmd, bg=TROUGH, fg=GREY, activebackground=GREEN,
                         activeforeground=BG, relief="flat", bd=0, padx=8, pady=3, font=_FONT, takefocus=0,
                         disabledforeground="#555555")

    def _section(self, parent, sec):
        f = tk.Frame(parent, bg=BG, highlightbackground=TROUGH, highlightthickness=1)
        _text(f, text=sec.upper().replace("_", " "), fg=GREEN).pack(anchor="w", padx=4)
        names = LAYOUT[sec]
        if sec == "matrix":
            grid = tk.Frame(f, bg=BG)
            grid.pack(anchor="w")
            for i in range(8):
                slot = tk.Frame(grid, bg=BG)
                slot.grid(row=i // 3, column=i % 3, padx=4, pady=1, sticky="w")
                sels = tk.Frame(slot, bg=BG)
                sels.pack(side="left")
                for k in ("src", "dst"):
                    self._make(sels, f"mod{i}_{k}").pack(anchor="w", pady=1)
                self._make(slot, f"mod{i}_amt").pack(side="left")
            return f
        top, row = tk.Frame(f, bg=BG), tk.Frame(f, bg=BG)
        top.pack(anchor="w", padx=2)
        row.pack(anchor="w")
        for n in names:
            w = self._make(top if widget_kind(n) == "selector" else row, n)
            w.pack(side="left", padx=2)
        return f

    def _make(self, parent, name):
        spec = REGISTRY[name]
        if widget_kind(name) == "selector":
            off = 1 if name == "polyphony" else 0
            w = Selector(parent, _labels(name), 0, lambda i, n=name, o=off: self._user(n, i + o))
        else:
            w = Knob(parent, KnobModel(spec, spec.default), lambda v, n=name: self._user(n, v),
                     label=_short(name), on_context=lambda e, n=name: self._context(n))
        self.widgets[name] = w
        return w

    # ---- engine <-> widgets -----------------------------------------------
    def _user(self, name, value):
        self._last = name
        self.engine.set_param(name, value)

    def _show(self, name, value):
        w = self.widgets.get(name)
        if isinstance(w, Selector):
            w.set_index(round(value) - (1 if name == "polyphony" else 0))
        elif w is not None:
            w.set_value(value)

    def _push_snapshot(self):
        snap = self.engine.snapshot()
        for n in self.widgets:
            self._show(n, snap[n])

    def apply_param(self, name, value):
        """Thread-safe: engine now, widget refresh on the next Tk poll."""
        self.engine.set_param(name, value)
        if not self._closed:
            self._pending.put((name, value))

    # ---- LEARN -------------------------------------------------------------
    def _context(self, name):
        self._last = name
        if self.midi_map is not None:
            self._arm(name)

    def _toggle_learn(self):
        if self.midi_map is None:
            return
        if self._armed_by_us:
            self._arm(None)
        elif self._last is None:
            self.status.set("LEARN: touch a control first")
        else:
            self._arm(self._last)

    def _arm(self, name):
        old = self._armed_by_us
        if old and isinstance(self.widgets.get(old), Knob):
            self.widgets[old].highlight(False)
        self.midi_map.arm(name)
        self._armed_by_us = name
        if name and isinstance(self.widgets[name], Knob):
            self.widgets[name].highlight(True)
        self.status.set(f"LEARN: {name} — move a CC" if name else "")

    def _learn_finished(self):
        name, self._armed_by_us = self._armed_by_us, None
        if isinstance(self.widgets.get(name), Knob):
            self.widgets[name].highlight(False)
        try:
            self.midi_map.save()
            self.status.set(f"LEARN: {name} bound")
        except OSError as e:
            self.status.set(f"LEARN: could not save map ({e.strerror or e})")

    # ---- header choosers -----------------------------------------------------
    def _set_dev_text(self):
        d = self._devs
        if self.stream is None or not d:
            text = "AUDIO: none"
        elif self._dev_i is None:
            act = getattr(self.stream, "active_device", None)
            name = next((x.name for x in d if x.index == act), None)
            text = f"AUDIO: {name[:28]}" if name else "AUDIO: auto"
        else:
            text = f"AUDIO: {d[self._dev_i].name[:28]}"
        self.dev_btn.configure(text=text, state="normal" if self.stream is not None and len(d) > 1 else "disabled")

    def _cycle_device(self):
        s = self.stream
        if s is None:
            return
        old = self._dev_i
        self._dev_i = ((old if old is not None else -1) + 1) % len(self._devs)
        s.close()  # restartable: start() reopens on the new device
        s.device_index = self._devs[self._dev_i].index
        try:
            s.start()
        except RuntimeError as e:
            self.status.set(str(e)[:80])
        self._set_dev_text()

    def on_midi_port(self, cb, ports=()):
        """Register ``cb(port_name)`` (called on the Tk thread) and the ports to cycle through."""
        self._midi_cb, self._midi_ports, self._midi_i = cb, list(ports), 0
        self._set_midi_text()

    def _set_midi_text(self):
        p = self._midi_ports
        self.midi_btn.configure(text=f"MIDI: {p[self._midi_i][:24]}" if p else "MIDI: none",
                                state="normal" if p and self._midi_cb else "disabled")

    def _cycle_midi(self):
        self._midi_i = (self._midi_i + 1) % len(self._midi_ports)
        self._set_midi_text()
        self._midi_cb(self._midi_ports[self._midi_i])

    # ---- poll / close ------------------------------------------------------
    def _poll(self):
        if self._closed:
            return
        while True:
            try:
                self._show(*self._pending.get_nowait())
            except queue.Empty:
                break
            except (ValueError, OverflowError):
                pass  # a non-finite value must never stop the poll loop
        notes = self.engine.vm.active_notes()
        self.keyboard.set_active(notes)
        self.voices.set(f"VOICES {len(notes)}")
        self.xruns.set(f"XRUNS {getattr(self.stream, 'xruns', 0)}")
        if self._armed_by_us and self.midi_map.learning is None:
            self._learn_finished()
        self._after = self.root.after(POLL_MS, self._poll)

    def close(self):
        """Idempotent. Destroys the window only; engine/stream/MIDI stay with their owner."""
        if self._closed:
            return
        self._closed = True
        self.scope.stop()
        try:
            if self._after:
                self.root.after_cancel(self._after)
            self.root.destroy()
        except tk.TclError:
            pass


def build_app(engine, stream=None, midi_map=None) -> App:
    return App(engine, stream, midi_map)
