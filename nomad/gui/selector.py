"""Segmented button selector (no dropdowns)."""

from __future__ import annotations

import tkinter as tk

BG, GREEN, GREY, TROUGH = "#101310", "#55e69a", "#999999", "#29342b"


class Selector(tk.Frame):
    """Row of flat buttons; click picks one. on_change(index) fires on user clicks only
    (select); set_index() updates silently."""

    def __init__(self, parent, labels, index, on_change):
        super().__init__(parent, bg=BG)
        self.labels, self.on_change = list(labels), on_change
        self._btns = [
            tk.Button(self, text=t, command=lambda i=i: self.select(i), relief="flat",
                      bd=0, padx=6, pady=2, font=("TkDefaultFont", 8), takefocus=0,
                      activebackground=TROUGH, activeforeground=GREEN)
            for i, t in enumerate(self.labels)
        ]
        for b in self._btns:
            b.pack(side="left", padx=1)
        self.index = 0
        self.set_index(index)

    def set_index(self, i: int) -> None:
        self.index = min(max(int(i), 0), len(self.labels) - 1)
        for k, b in enumerate(self._btns):
            on = k == self.index
            b.configure(bg=GREEN if on else TROUGH, fg=BG if on else GREY,
                        activebackground=GREEN if on else TROUGH)

    def select(self, i: int) -> None:
        self.set_index(i)
        self.on_change(self.index)
