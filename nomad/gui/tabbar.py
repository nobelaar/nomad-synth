"""Browser-style instrument tabs: select, close, and a '+' menu to create instances.

Only the Tk thread touches it. It holds no state about instruments: ``App``
rebuilds it with ``refresh`` after every change (create, close, select, clock
source), so it can never disagree with the manager.
"""

from __future__ import annotations

import tkinter as tk

from nomad.gui.knob import BG, GREEN, GREY, TROUGH

_FONT = ("TkDefaultFont", 8)
_BTN = dict(relief="flat", bd=0, padx=6, pady=3, font=_FONT, takefocus=0, activebackground=GREEN,
            activeforeground=BG)


class TabBar(tk.Frame):
    def __init__(self, parent, types, on_select, on_close, on_new, register_help=None):
        """``types``: kind -> object with ``.label``; callbacks take an instrument id / a kind."""
        super().__init__(parent, bg=BG)
        self._on_select, self._on_close, self._on_new, self._help = on_select, on_close, on_new, register_help
        self.menu = tk.Menu(self, tearoff=0, bg=TROUGH, fg=GREY, activebackground=GREEN,
                            activeforeground=BG, font=_FONT)
        for kind, t in types.items():
            self.menu.add_command(label=t.label, command=lambda k=kind: self._on_new(k))
        self.tabs: dict[int, tuple[tk.Button, tk.Button]] = {}
        self.new_btn = tk.Button(self, text="+", command=self._popup, bg=TROUGH, fg=GREY, **_BTN)
        self.new_btn.pack(side="left", padx=(4, 0))
        self._reg(self.new_btn, "tab_new")

    def _reg(self, widget, key):
        if self._help is not None:
            self._help(widget, key)

    def _popup(self) -> None:
        self.menu.tk_popup(self.new_btn.winfo_rootx(), self.new_btn.winfo_rooty() + self.new_btn.winfo_height())

    def menu_labels(self) -> list[str]:
        return [self.menu.entrycget(i, "label") for i in range(self.menu.index("end") + 1)]

    def refresh(self, items) -> None:
        """``items``: ``(instrument id, text, selected)`` in tab order."""
        for label, close in self.tabs.values():
            label.destroy()
            close.destroy()
        self.tabs = {}
        for iid, text, selected in items:
            bg, fg = (GREEN, BG) if selected else (TROUGH, GREY)
            label = tk.Button(self, text=text, command=lambda i=iid: self._on_select(i), bg=bg, fg=fg, **_BTN)
            close = tk.Button(self, text="×", command=lambda i=iid: self._on_close(i), bg=bg, fg=fg,
                              **{**_BTN, "padx": 3})
            label.pack(side="left", padx=(4, 0), before=self.new_btn)
            close.pack(side="left", padx=(0, 0), before=self.new_btn)
            self._reg(label, "tab_select")
            self._reg(close, "tab_close")
            self.tabs[iid] = (label, close)
