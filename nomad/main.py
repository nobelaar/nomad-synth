"""NOMAD SYNTH entrypoint: python -m nomad.main [options]."""
from __future__ import annotations

import argparse
import os
import signal
import sys
from pathlib import Path

import numpy as np

from nomad.audio.scope import ScopeBuffer
from nomad.audio.stream import AudioStream
from nomad.engine.engine import Engine
from nomad.midi.mapping import MidiMap
from nomad.presets import PresetStore
from nomad.midi.midi_input import MidiEvent, MidiInput, MidiRouter


def _unit(s: str) -> float:
    v = float(s)
    if not 0.0 <= v <= 1.0:
        raise argparse.ArgumentTypeError("must be between 0 and 1")
    return v


def _note(s: str) -> int:
    v = int(s)
    if not 0 <= v <= 127:
        raise argparse.ArgumentTypeError("must be between 0 and 127")
    return v


def default_config_dir() -> Path:
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return Path(base) / "nomad-synth"


def make_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="python -m nomad.main", description="NOMAD SYNTH")
    ap.add_argument("--no-midi", action="store_true", help="do not open any MIDI input")
    ap.add_argument("--device", type=int, metavar="N", help="audio output device index (default: auto)")
    ap.add_argument("--demo-note", type=_note, metavar="N", help="play MIDI note N (velocity 0.8) after startup")
    ap.add_argument("--dry-run", action="store_true", help="build the engine only: no audio stream, window or MIDI")
    ap.add_argument("--volume", type=_unit, metavar="F", help="master volume 0..1 (default: engine default)")
    ap.add_argument("--config-dir", type=Path, default=None, metavar="PATH",
                    help="where midi-map.json lives (default: $XDG_CONFIG_HOME or ~/.config, + /nomad-synth)")
    return ap


def _warm_up(sr, blocks=16):
    """Render a throwaway engine so cold paths are paid before the stream starts."""
    e = Engine(sr)
    for n in (48, 60, 72):
        e.note_on(n, 1.0)
    out = np.zeros(256)
    for _ in range(blocks):
        e.render(out)


class Runtime:
    def __init__(self, args, engine, stream, midi_map, midi_in, router):
        self.args, self.engine, self.stream = args, engine, stream
        self.midi_map, self.midi_in, self.router = midi_map, midi_in, router
        self.app = None
        self.presets = None
        self.last_stats: dict | None = None
        self._closed = False
        self._stop = False
        self._old_sigs: dict = {}

    @property
    def closed(self) -> bool:
        return self._closed

    # ---- wiring ------------------------------------------------------------
    def _on_cc(self, cc, value, channel):
        """rtmidi thread: learned/mapped CC -> GUI + engine (apply_param is thread-safe)."""
        r = self.midi_map.handle(MidiEvent("cc", cc=cc, value=value, channel=channel, port=self.midi_in._name))
        if r is not None:
            (self.app.apply_param if self.app else self.engine.set_param)(*r)

    def _list_ports(self):
        # real controllers first: ALSA's 'Midi Through' is a loopback that never sends
        return sorted(self.midi_in.list_ports(), key=lambda p: "Midi Through" in p)

    def _open_midi(self, name=None):
        """Tk thread. Close first so no events arrive while the router resets."""
        self.midi_in.close()
        self.router.reset()
        try:
            self.midi_in.open(name)
        except RuntimeError as e:
            if self.app:
                self.app.status.set(str(e)[:80])
            else:
                print(f"midi: {e}", file=sys.stderr)

    def start(self) -> None:
        """Non-blocking: start audio, build the window, open MIDI. See run()."""
        a = self.args
        if a.dry_run:
            self._demo()
            return
        _warm_up(self.engine.sr)  # first-use numpy/scipy costs must not land in the audio callback
        from nomad.gui.app import build_app
        self.app = app = build_app(self.engine, self.stream, self.midi_map, self.presets)
        app.root.update()  # Tk widget construction holds the GIL: finish it BEFORE the stream starts
        try:
            self.stream.start()
        except RuntimeError as e:
            print(f"audio: {e}", file=sys.stderr)
            self.stream = app.stream = None  # window stays open, header says AUDIO: none
        app.on_target_change = self.router.reset  # release MIDI-held notes when the target Synth changes
        app.refresh_audio()  # the label shows the device the stream actually resolved
        app.root.protocol("WM_DELETE_WINDOW", self.close)
        ports = [] if a.no_midi else self._list_ports()
        app.on_midi_port(self._open_midi, ports, rescan=None if a.no_midi else self._list_ports)
        if ports:
            self._open_midi(ports[0])
        self._demo()
        for s in (signal.SIGINT, signal.SIGTERM):
            self._old_sigs[s] = signal.signal(s, lambda *_: setattr(self, "_stop", True))
        app.root.after(100, self._watch_signal)

    def _watch_signal(self):
        if self._closed:
            return
        if self._stop:
            self.close()
        else:
            self.app.root.after(100, self._watch_signal)

    def _demo(self):
        n = self.args.demo_note
        if n is None:
            return
        if self.app:
            self.app.root.after(300, lambda: self.engine.note_on(n, 0.8))
        else:
            self.engine.note_on(n, 0.8)

    def run(self) -> None:
        try:
            self.start()
            if self.app:
                self.app.root.mainloop()
        finally:
            self.close()

    # ---- shutdown ----------------------------------------------------------
    def close(self) -> None:
        """Idempotent. MIDI first (no more events), then audio, then the window."""
        if self._closed:
            return
        self._closed = True
        s = self.stream
        if s is not None:
            self.last_stats = {"callbacks": s.callbacks, "xruns": s.xruns, "errors": s.errors}
        steps = [self.midi_in.close, (s.close if s is not None else None),
                 (self.app.close if self.app else None)]
        for step in steps:
            if step is None:
                continue
            try:
                step()
            except Exception as e:  # one failing step must not block the others
                print(f"shutdown: {e}", file=sys.stderr)
        for sig, old in self._old_sigs.items():
            try:
                signal.signal(sig, old)
            except (ValueError, TypeError):
                pass


def build_runtime(argv=None) -> Runtime:
    """Parse args and build all parts. No audio/MIDI/window is opened until start()."""
    args = make_parser().parse_args(argv)
    engine = Engine(48000)
    if args.volume is not None:
        engine.set_param("master_volume", args.volume)
    stream = None if args.dry_run else AudioStream(engine, device_index=args.device, scope=ScopeBuffer(engine.sr))
    cfg = args.config_dir if args.config_dir is not None else default_config_dir()
    midi_map = MidiMap(cfg / "midi-map.json")
    midi_map.load()
    rt = Runtime(args, engine, stream, midi_map, None, None)
    rt.presets = PresetStore(cfg / "presets")
    rt.router = MidiRouter(engine, on_cc=rt._on_cc)
    rt.midi_in = MidiInput(rt.router.handle)
    return rt


def main(argv=None) -> int:
    rt = build_runtime(argv)
    rt.run()
    if rt.last_stats:
        print("stream: callbacks={callbacks} xruns={xruns} errors={errors}".format(**rt.last_stats), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
