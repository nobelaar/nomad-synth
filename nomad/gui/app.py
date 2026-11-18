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
from nomad.engine.params import REGISTRY
from nomad.gui.drums_view import DrumView
from nomad.gui.knob import BG, GREEN, GREY, TROUGH, Knob, KnobModel
from nomad.gui.help import help_for
from nomad.gui.keyboard import Keyboard
from nomad.presets import PresetStore
from nomad.gui.scope_view import ScopeView
from nomad.gui.selector import Selector
from nomad.engine.manager import InstrumentManager
from nomad.engine.registry import INSTRUMENT_TYPES
from nomad.gui.strip import InstanceStrip
from nomad.gui.tabbar import TabBar
from nomad.gui.synth_panel import (HIDDEN, LAYOUT, SynthPanel, _FONT, _labels, _short, _text,  # noqa: F401
                                   make_control, widget_kind)

POLL_MS = 100


def panel_ready(panel) -> bool:
    return getattr(panel, "ready", True)
HELP_TAG = "NomadHelp"   # bindtag put first on every widget while help mode is on
HELP_DELAY_MS = 150
HELP_WRAP = 330


class App:
    def __init__(self, engine, stream=None, midi_map=None, presets=None):
        self.engine, self.stream, self.midi_map = engine, stream, midi_map
        self.presets = presets if presets is not None else PresetStore(None)
        self._global_widgets: dict[str, Knob | Selector] = {}  # app-level controls (master volume)
        self.strips: dict[int, InstanceStrip] = {}
        self.panels: dict[int, tk.Misc] = {}   # instance id -> persistent panel (Synth/Drum view)
        self.selected = None                   # the instance being edited (NOT the only one running)
        self.on_target_change = None           # set by the runtime: release MIDI-held notes before retargeting
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
        self._midi_rescan, self._midi_open = None, False
        self.status = tk.StringVar(value="")
        self.help_on = False
        self._help_keys: dict[str, str] = {}   # widget path -> help key
        self._help_cursors: dict[tk.Misc, str] = {}
        self._help_after = None
        self._help_shown: str | None = None
        self._menu = tk.Menu(root, tearoff=0, bg=TROUGH, fg=GREY, activebackground=GREEN,
                             activeforeground=BG, disabledforeground="#555555", font=_FONT)
        root.bind_all("<ButtonPress-1>", self._close_menu, add="+")
        root.bind_all("<Escape>", self._close_menu, add="+")
        self.manager = InstrumentManager(engine, schedule=root.after)
        self._build()
        self._build_help()
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
        self.left = left
        self.side = tk.Frame(right, bg=BG)  # per-instance VOICE sections live here (below scope + master)
        self.scope_frame = tk.Frame(right, bg=BG, width=250, height=150)
        self.scope_frame.pack_propagate(False)
        self.scope_frame.pack(anchor="nw", pady=2)
        self.scope = ScopeView(self.scope_frame, self.scope_buffer, 250, 150, sr=self.engine.sr)
        self.scope.pack()
        master = tk.Frame(right, bg=BG, highlightbackground=TROUGH, highlightthickness=1)
        _text(master, text="MASTER", fg=GREEN).pack(anchor="w", padx=4)
        self._global_widgets["master_volume"] = make_control(master, "master_volume", self._user, self._context,
                                                              self._help_register)
        self._global_widgets["master_volume"].pack(anchor="w", padx=2)
        master.pack(anchor="nw", pady=2)
        self.side.pack(anchor="nw", pady=2)
        self.keyboard_frame = tk.Frame(root, bg=BG, width=1350, height=84)
        self.keyboard_frame.pack_propagate(False)
        self.keyboard_frame.grid(row=2, column=0, columnspan=2, padx=6, pady=4)
        self.keyboard = Keyboard(self.keyboard_frame, self.engine.note_on, self.engine.note_off, octaves=6)
        self.keyboard.pack()
        self._help_register(self.keyboard, "keyboard")
        self._help_register(self.scope, "scope")
        for inst in self.manager.instances:
            self.panels[inst.id] = self._make_panel(inst, eager=True)
        self.select(self.engine.synth)

    # ---- instruments and tabs ----------------------------------------------------
    @property
    def synth_view(self):
        """Panel of the synth that receives MIDI / the on-screen keyboard (the last selected Synth)."""
        return self.panels.get(self.engine.synth.id)

    @property
    def drums_view(self):
        return self.panels.get(self.engine.drums.id)

    def _make_panel(self, inst, eager=True):
        if inst.kind == "synth":
            panel = SynthPanel(self.left, self, inst, side=self.side, eager=eager)
        else:
            panel = DrumView(self.left, inst, register_help=self._help_register,
                             on_context=lambda n, e, i=inst: self._drum_context(i, n, e), eager=eager)
        panel.grid(row=0, column=0, sticky="nw", padx=2, pady=2)
        if not hasattr(panel, "side_frame"):  # Machine Drum: its strip lives in the right column too
            panel.side_frame = tk.Frame(self.side, bg=BG)
            panel.side_frame.grid(row=0, column=0, sticky="nw")
        strip = InstanceStrip(panel.side_frame, self, inst, panel, with_bpm=inst.kind == "synth",
                              register_help=self._help_register)
        strip.pack(anchor="nw", pady=2, before=next(iter(panel.side_frame.pack_slaves()), None))
        self.strips[inst.id] = strip
        self._set_visible(panel, False)
        return panel

    @staticmethod
    def _set_visible(panel, on):
        if hasattr(panel, "set_visible"):
            panel.set_visible(on)
        else:
            for f in (panel, panel.side_frame):
                f.grid() if on else f.grid_remove()

    def add_instrument(self, kind, defer=True):
        """Create an instance (audible at once) and its panel; the panel is assembled in small ``after`` steps
        (``defer=False`` builds it synchronously, for tests) and selected when complete."""
        inst = self.manager.add(kind)
        panel = self._make_panel(inst, eager=not defer)
        if not defer or panel_ready(panel):
            self._panel_ready(inst, panel)
        else:
            self.refresh_tabs()
            self.root.after(1, lambda: self._build_step(inst, panel))
        return inst

    def _build_step(self, inst, panel):
        if self._closed or inst not in self.manager.instances:
            return
        if panel.build_next():
            self.root.after(1, lambda: self._build_step(inst, panel))
        else:
            self._panel_ready(inst, panel)

    def _panel_ready(self, inst, panel):
        self.panels[inst.id] = panel
        if self.selected is inst:  # it was selected while still being built: show it now
            self.selected = None
        self.select(inst)

    def select_id(self, iid):
        inst = next((i for i in self.manager.instances if i.id == iid), None)
        if inst is not None and iid in self.panels:  # a panel still being built cannot be selected yet
            self.select(inst)

    def select(self, inst):
        """Choose the instrument being edited. Never touches what is playing."""
        if inst is self.selected and inst is not None:
            self.refresh_tabs()
            return
        if inst is not None and inst.kind == "synth" and inst is not self.engine.synth:
            self._retarget(inst)
        for iid, panel in self.panels.items():
            self._set_visible(panel, inst is not None and iid == inst.id)
        self.selected = inst
        synth = inst is not None and inst.kind == "synth"
        self.keyboard_frame.grid() if synth else self.keyboard_frame.grid_remove()
        if inst is not None and inst.id in self.panels:
            self.panels[inst.id].refresh()
        self.status.set("" if synth or inst is None else "DRUMS: PLAY para arrancar, shift+click acentúa")
        self.refresh_tabs()

    def _retarget(self, inst):
        """MIDI and the on-screen keyboard follow the selected Synth: release what is held on the old one first."""
        old = self.engine.synth
        if self.on_target_change is not None:
            self.on_target_change()  # router.reset() releases via engine.note_off -> still the old target
        for note, held in enumerate(old._live):
            if held:
                old.note_off(note)
        self.engine.synth = inst  # each panel keeps its own widget state: nothing to re-push

    def close_instance(self, inst):
        instances = list(self.manager.instances)
        if inst not in instances:
            return
        idx = instances.index(inst)
        was_selected = inst is self.selected
        if inst is self.engine.synth and self.on_target_change is not None:
            self.on_target_change()
        self.manager.remove(inst)
        panel = self.panels.pop(inst.id, None)
        self.strips.pop(inst.id, None)
        if panel is not None:
            if hasattr(panel, "side_frame"):
                panel.side_frame.destroy()
            panel.destroy()
        rest = list(self.manager.instances)
        if inst is self.engine.synth:  # keep MIDI / keyboard pointing at a live Synth if one remains
            alt = next((i for i in rest if i.kind == "synth" and i.id in self.panels), None)
            if alt is not None:
                self.engine.synth = alt
        if was_selected:
            self.selected = None
            ready = [i for i in rest if i.id in self.panels]  # never fall back to a panel still being built
            self.select(min(ready, key=lambda i: abs(instances.index(i) - idx)) if ready else None)
        else:
            self.refresh_tabs()

    def refresh_tabs(self):
        items = []
        for inst in self.manager.instances:
            text = inst.name
            if self.manager.followers(inst):
                text += " ●"
            if inst.source is not None:
                text += f" ⇄ {inst.source.name}"
            items.append((inst.id, text, inst is self.selected))
        self.tabbar.refresh(items)

    def _drum_context(self, inst, name, event):
        """Right-click on a drum knob: only "default value" (no MIDI learn yet)."""
        self._menu.delete(0, "end")
        self._menu.add_command(label="Valor por defecto", command=lambda: self.panels[inst.id].reset(name))
        if event is not None:
            self._menu.tk_popup(event.x_root, event.y_root)

    def _build_header(self, h):
        tk.Label(h, text="NOMAD SYNTH", bg=BG, fg=GREEN, font=("TkDefaultFont", 14, "bold")).pack(side="left", padx=8)
        self.tabbar = TabBar(h, INSTRUMENT_TYPES, self.select_id,
                             lambda iid: self.close_instance(next(i for i in self.manager.instances if i.id == iid)),
                             self.add_instrument, register_help=self._help_register)
        self.tabbar.pack(side="left", padx=(4, 6))
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
            lbl = tk.Label(h, textvariable=v, bg=BG, fg=GREY, font=_FONT, width=10)
            lbl.pack(side="left", padx=6)
            self._help_register(lbl, "voices" if v is self.voices else "xruns")
        self.learn_btn = self._btn(h, self._toggle_learn, "LEARN")
        if self.midi_map is None:
            self.learn_btn.configure(state="disabled")
            self.status.set("LEARN unavailable: no MIDI map")
        self.learn_btn.pack(side="left", padx=6)
        self.help_btn = self._btn(h, self.toggle_help, "?")
        self.help_btn.pack(side="left", padx=6)
        for w, key in ((self.learn_btn, "learn"), (self.dev_btn, "audio"), (self.midi_btn, "midi"),
                       (self.help_btn, "help")):
            self._help_register(w, key)
        tk.Label(h, textvariable=self.status, bg=BG, fg=GREEN, font=_FONT).pack(side="left", padx=6)
        return h

    @staticmethod
    def _btn(parent, cmd, text=""):
        return tk.Button(parent, text=text, command=cmd, bg=TROUGH, fg=GREY, activebackground=GREEN,
                         activeforeground=BG, relief="flat", bd=0, padx=8, pady=3, font=_FONT, takefocus=0,
                         disabledforeground="#555555")

    @property
    def widgets(self) -> dict:
        """All controls by parameter name: the selected Synth panel's plus the app-level ones."""
        panel = self.synth_view
        return {**(panel.widgets if panel is not None else {}), **self._global_widgets}

    # ---- help mode -----------------------------------------------------------
    def _help_register(self, widget, key):
        self._help_keys[str(widget)] = key

    def _build_help(self):
        r = self.root
        for ev in ("<ButtonPress>", "<ButtonRelease>", "<Motion>", "<MouseWheel>"):
            r.bind_class(HELP_TAG, ev, lambda e: "break")  # controls paused
        r.bind_class(HELP_TAG, "<Enter>", self._help_enter)
        r.bind_all("<F1>", lambda e: self.toggle_help(), add="+")
        r.bind_all("<Escape>", lambda e: self.toggle_help() if self.help_on else None, add="+")
        win = self._help_win = tk.Toplevel(r, bg=GREEN)
        win.withdraw()
        win.overrideredirect(True)
        box = tk.Frame(win, bg=BG, padx=10, pady=8)
        box.pack(padx=1, pady=1)
        self._help_title = tk.Label(box, bg=BG, fg=GREEN, font=("TkDefaultFont", 10, "bold"),
                                    justify="left", anchor="w")
        self._help_title.pack(anchor="w")
        self._help_body = tk.Label(box, bg=BG, fg="#dddddd", font=("TkDefaultFont", 9),
                                   justify="left", anchor="w", wraplength=HELP_WRAP)
        self._help_body.pack(anchor="w", pady=(4, 0))

    def _walk(self, w):
        yield w
        for c in w.winfo_children():
            if c is not self._help_win:
                yield from self._walk(c)

    def toggle_help(self):
        """Help mode: question-mark cursor, hovering explains, controls don't respond."""
        self.help_on = on = not self.help_on
        self._menu.unpost()
        for w in self._walk(self.root):
            if w is self.help_btn:
                continue
            tags = tuple(t for t in w.bindtags() if t != HELP_TAG)
            w.bindtags(((HELP_TAG,) + tags) if on else tags)
            if on:
                self._help_cursors[w] = w.cget("cursor")
                w.configure(cursor="question_arrow")
            elif w in self._help_cursors:
                w.configure(cursor=self._help_cursors.pop(w))
        self.help_btn.configure(bg=GREEN if on else TROUGH, fg=BG if on else GREY)
        self.status.set("AYUDA: pasá el mouse por un control (F1 / Esc para salir)" if on else "")
        if not on:
            if self._help_after:
                self.root.after_cancel(self._help_after)
                self._help_after = None
            self._help_win.withdraw()
            self._help_shown = None

    def _help_key_for(self, w):
        while w is not None:
            key = self._help_keys.get(str(w))
            if key:
                return key
            w = w.master
        return None

    def _help_enter(self, e):
        key = self._help_key_for(e.widget)
        if key is None or key == self._help_shown:
            return
        target = e.widget
        while str(target) not in self._help_keys:
            target = target.master
        if self._help_after:
            self.root.after_cancel(self._help_after)
        self._help_after = self.root.after(HELP_DELAY_MS, lambda: self._help_show(key, target))

    def _help_show(self, key, widget):
        """Fill and place the explanation window next to ``widget``; it stays until another hover."""
        self._help_after = None
        if not self.help_on:
            return
        title, body = help_for(key)
        self._help_title.configure(text=title)
        self._help_body.configure(text=body)
        win = self._help_win
        win.update_idletasks()
        w, h = win.winfo_reqwidth(), win.winfo_reqheight()
        sw, sh = self.root.winfo_screenwidth(), self.root.winfo_screenheight()
        x = widget.winfo_rootx() + widget.winfo_width() + 8
        if x + w > sw:
            x = widget.winfo_rootx() - w - 8
        y = min(widget.winfo_rooty(), sh - h - 4)
        win.geometry(f"+{max(0, x)}+{max(0, y)}")
        win.deiconify()
        win.lift()
        self._help_shown = key

    # ---- engine <-> widgets -----------------------------------------------
    def _user(self, name, value):
        self._last = name
        self.engine.set_param(name, value)  # master_volume -> mix; everything else -> the target synth

    def _show(self, name, value):
        w = self._global_widgets.get(name)
        if w is not None:
            w.set_value(value)
        elif self.synth_view is not None:
            self.synth_view.show(name, value)

    def _push_snapshot(self):
        snap = self.engine.snapshot()
        for n in self.widgets:
            self._show(n, snap[n])

    def apply_param(self, name, value):
        """Thread-safe: engine now, widget refresh on the next Tk poll."""
        self.engine.set_param(name, value)
        if not self._closed:
            self._pending.put((name, value))

    # ---- right-click menu / LEARN -------------------------------------------
    def _menu_items(self, name):
        """(label, callback) for the right-click menu; callback None = disabled."""
        items = [("Valor por defecto", lambda: self._reset(name))]
        m = self.midi_map
        if m is None:
            items.append(("MIDI learn", None))
        elif self._armed_by_us == name:
            items.append(("Cancelar learn", lambda: self._arm(None)))
        else:
            items.append(("MIDI learn", lambda: self._arm(name)))
        if m is not None and name in m.bindings.values():
            items.append(("Quitar MIDI", lambda: self._unbind(name)))
        return items

    def _context(self, name, event):
        self._last = name
        menu = self._menu
        menu.delete(0, "end")
        for label, cb in self._menu_items(name):
            menu.add_command(label=label, command=cb or (lambda: None),
                             state="normal" if cb else "disabled")
        if event is not None:
            # No grab_release(): on X11 it drops the menu's grab and the menu
            # then stays open after a click elsewhere.
            menu.tk_popup(event.x_root, event.y_root)

    def _close_menu(self, event=None):
        if event is None or event.widget is not self._menu:
            self._menu.unpost()

    def _reset(self, name):
        value = REGISTRY[name].default
        self._show(name, value)
        self._user(name, value)

    def _unbind(self, name):
        self.midi_map.unbind(name)
        try:
            self.midi_map.save()
            self.status.set(f"MIDI: {name} desvinculado")
        except OSError as e:
            self.status.set(f"MIDI: could not save map ({e.strerror or e})")

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

    def refresh_audio(self):
        """Re-read the stream's resolved device (call after the stream started or failed)."""
        self._set_dev_text()

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

    def on_midi_port(self, cb, ports=(), rescan=None):
        """Register ``cb(port_name)`` (called on the Tk thread) and the ports to cycle through.

        ``rescan()`` (optional) returns the current port list; the button calls it on every
        click so a keyboard plugged in after launch is found.
        """
        self._midi_cb, self._midi_ports, self._midi_i = cb, list(ports), 0
        self._midi_rescan, self._midi_open = rescan, bool(ports)
        self._set_midi_text()

    def _set_midi_text(self):
        p = self._midi_ports
        self.midi_btn.configure(text=f"MIDI: {p[self._midi_i][:24]}" if p else "MIDI: none",
                                state="normal" if self._midi_cb and (p or self._midi_rescan) else "disabled")

    def _cycle_midi(self):
        cur = self._midi_ports[self._midi_i] if self._midi_ports else None
        if self._midi_rescan is not None:
            self._midi_ports = list(self._midi_rescan())
        if not self._midi_ports:
            self.status.set("MIDI: no ports found")
            self._set_midi_text()
            return
        if self._midi_open and cur in self._midi_ports:
            self._midi_i = (self._midi_ports.index(cur) + 1) % len(self._midi_ports)
        else:  # nothing open yet (or the port vanished): start at the preferred one
            self._midi_i = 0
        self._midi_open = True
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
        notes = self.engine.snapshot()["active_notes"]
        self.keyboard.set_active(notes)
        self.manager.collect()  # old topologies / closed instances die on this thread, not in the callback
        sel = self.panels.get(self.selected.id) if self.selected is not None else None
        if sel is not None:
            sel.refresh()  # hidden panels are not repainted: the Tk thread has to stay cheap next to the audio
        for iid in list(self.panels):
            self.strips[iid].refresh()
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


def build_app(engine, stream=None, midi_map=None, presets=None) -> App:
    return App(engine, stream, midi_map, presets)
