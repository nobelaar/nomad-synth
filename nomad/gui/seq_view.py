"""Step editor for the synth's sequencer, embedded in the synth panel.

Only the Tk thread touches widgets. It writes ``StepSequencer`` arrays and the
shared ``Clock`` through plain stores; ``refresh`` runs from the 100 ms poll.
"""

from __future__ import annotations

import tkinter as tk

from nomad.engine.clock import STEPS
from nomad.engine.sequencer import NONE
from nomad.gui.drums_view import ACCENT, PLAYHEAD, _BTN
from nomad.gui.knob import BG, GREEN, GREY, TROUGH

_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
ACCENT_VEL, NORMAL_VEL = 127, 100
DIVS = ((0.25, "1/16"), (0.5, "1/8"), (1.0, "1/4"), (2.0, "1/2"), (4.0, "1 bar"))  # step length in beats


def note_name(n: int) -> str:
    return f"{_NAMES[n % 12]}{n // 12 - 1}"


class SeqView(tk.Frame):
    def __init__(self, parent, synth, register_help=None):
        super().__init__(parent, bg=BG)
        self.synth, self.seq, self.clock, self._help = synth, synth.sequencer, synth.clock, register_help
        sequencer = self.seq
        self._rec = False
        top = tk.Frame(self, bg=BG)
        top.pack(anchor="w", padx=2, pady=2)
        self.seq_btn = tk.Button(top, text="SEQ", command=self.toggle_enabled, **_BTN)
        self.seq_btn.pack(side="left", padx=(0, 4))
        self.play_btn = tk.Button(top, text="PLAY", command=self.toggle_transport, **_BTN)
        self.play_btn.pack(side="left", padx=(0, 8))
        self.rec_btn = tk.Button(top, text="REC", command=self.toggle_rec, **_BTN)
        self.rec_btn.pack(side="left", padx=(0, 4))
        self.rest_btn = tk.Button(top, text="REST", command=self.rest, state="disabled", **_BTN)
        self.rest_btn.pack(side="left", padx=(0, 8))
        tk.Label(top, text="GATE", bg=BG, fg=GREY, font=("TkDefaultFont", 8)).pack(side="left")
        self._gate_var = tk.DoubleVar(self, value=round(sequencer.gate * 100))
        self._gate_var.trace_add("write", lambda *_: self.set_gate(self._gate_var.get() / 100.0))
        self.gate = tk.Scale(top, from_=5, to=100, orient="horizontal", length=90, showvalue=False,
                             variable=self._gate_var, bg=BG, fg=GREY,
                             troughcolor=TROUGH, highlightthickness=0, bd=0, sliderlength=14,
                             activebackground=GREEN, takefocus=0)
        self.gate.pack(side="left", padx=2)
        self.div_btn = tk.Button(top, text="", command=self.cycle_div, **_BTN)
        self.div_btn.pack(side="left", padx=(8, 0))
        self.len_btn = tk.Button(top, text="", command=self.cycle_len, **_BTN)
        self.len_btn.pack(side="left", padx=(4, 0))
        self.clear_btn = tk.Button(top, text="CLEAR", command=self.clear, **_BTN)
        self.clear_btn.pack(side="left", padx=8)
        grid = tk.Frame(self, bg=BG)
        grid.pack(anchor="w", padx=2)
        self.steps: list[tk.Button] = []
        for s in range(STEPS):
            b = tk.Button(grid, text="", width=4, **_BTN)
            b.pack(side="left", padx=1)
            b.bind("<Button-1>", lambda e, s=s: self.toggle_step(s, bool(e.state & 1)))
            b.bind("<Button-4>", lambda e, s=s: self.set_note(s, 12 if e.state & 1 else 1))
            b.bind("<Button-5>", lambda e, s=s: self.set_note(s, -12 if e.state & 1 else -1))
            b.bind("<MouseWheel>", lambda e, s=s: self.set_note(s, (12 if e.state & 1 else 1) * (1 if e.delta > 0 else -1)))
            self.steps.append(b)
            self._reg(b, "seq_steps")
        self._reg(self.seq_btn, "seq_toggle")
        self._reg(self.play_btn, "seq_transport")
        self._reg(self.gate, "seq_gate")
        self._reg(self.rec_btn, "seq_rec")
        self._reg(self.div_btn, "seq_div")
        self._reg(self.len_btn, "seq_len")
        self._reg(self.rest_btn, "seq_rest")
        self.refresh()

    def _reg(self, widget, key):
        if self._help is not None:
            self._help(widget, key)

    # ---- user actions ------------------------------------------------------
    def toggle_enabled(self) -> None:
        self.seq.enabled = not self.seq.enabled
        self.refresh()

    def set_transport_locked(self, locked: bool) -> None:
        self.play_btn.configure(state="disabled" if locked else "normal")

    def toggle_transport(self) -> None:
        self.synth.toggle()  # a command: the audio thread applies it at the next render
        self.refresh()

    def toggle_rec(self) -> None:
        self._rec = not self._rec  # intent; the audio thread owns the real flag and cursor
        self.synth.set_rec(self._rec)
        self.refresh()

    def rest(self) -> None:
        self.synth.rest()

    def cycle_div(self) -> None:
        cur = self.seq.grid.division
        i = min(range(len(DIVS)), key=lambda j: abs(DIVS[j][0] - cur))
        self.seq.grid.division = DIVS[(i + 1) % len(DIVS)][0]
        self.refresh()

    def cycle_len(self) -> None:
        self.seq.grid.length = self.seq.grid.length % STEPS + 1
        self.refresh()

    def set_gate(self, v: float) -> None:
        self.seq.gate = v

    def clear(self) -> None:
        self.seq.clear()
        self.refresh()

    def toggle_step(self, step: int, accent: bool = False) -> None:
        on, vel = bool(self.seq.on[step]), int(self.seq.vel[step])
        if accent:
            self.seq.set_step(step, not (on and vel == ACCENT_VEL), vel=ACCENT_VEL)
        else:
            self.seq.set_step(step, not on, vel=NORMAL_VEL if not on else None)
        self.refresh()

    def set_note(self, step: int, delta: int) -> None:
        self.seq.transpose(step, delta)
        self.refresh()

    # ---- drawing -----------------------------------------------------------
    def refresh(self) -> None:
        seq, playing = self.seq, self.clock.running
        pos = seq.position if playing else (seq.rec_step if self._rec else -1)
        for s, b in enumerate(self.steps):
            on, vel = seq.on[s], int(seq.vel[s])
            extra = int((seq.extra[s] != NONE).sum())
            live = s < seq.grid.length
            b.configure(text=note_name(int(seq.note[s])) + (f"+{extra}" if extra else ""),
                        bg=(ACCENT if vel == ACCENT_VEL else GREEN) if on and live else TROUGH,
                        fg=BG if on and live else GREY, highlightthickness=2,
                        highlightbackground=PLAYHEAD if s == pos else BG)
        self.seq_btn.configure(bg=GREEN if seq.enabled else TROUGH, fg=BG if seq.enabled else GREY)
        self.div_btn.configure(text="DIV " + dict((d, n) for d, n in DIVS).get(seq.grid.division, "?"))
        self.len_btn.configure(text=f"LEN {seq.grid.length}")
        self.rec_btn.configure(text=("REC·LIVE" if playing else "REC·STEP") if self._rec else "REC",
                               bg=ACCENT if self._rec else TROUGH, fg=BG if self._rec else GREY)
        self.rest_btn.configure(state="normal" if self._rec and not playing else "disabled")
        self.play_btn.configure(text="STOP" if playing else "PLAY",
                                bg=GREEN if playing else TROUGH, fg=BG if playing else GREY)
