"""sounddevice output stream. The callback only renders, mirrors to stereo,
feeds the scope and measures timing; it never raises and never allocates.

Xruns are counted two ways (the JACK host API often leaves `status` empty):
  1. PortAudio `status.output_underflow`;
  2. lateness: each callback's `currentTime` is compared with where it
     should be given the frames already delivered (anchor + frames/sr). A
     cumulative reference is used because PortAudio-over-JACK delivers
     blocks in bursts (a 1024-frame JACK period arrives as several smaller
     callbacks with ~equal timestamps), so per-callback gaps are meaningless.
     Arriving early moves the anchor earlier; arriving more than
     LATE_FACTOR blocks late counts one xrun and re-anchors. `max_late_ms`
     keeps the worst lateness of the session.
"""
from dataclasses import dataclass

import numpy as np
import sounddevice as sd

JACK = "JACK Audio Connection Kit"
LATE_FACTOR = 1.5


@dataclass(frozen=True)
class OutputDevice:
    index: int
    name: str
    hostapi: str
    samplerate: float
    channels: int


def list_output_devices() -> list[OutputDevice]:
    try:
        devs = sd.query_devices()
        apis = sd.query_hostapis()
        return [OutputDevice(int(d["index"]) if "index" in d else i, d["name"],
                             apis[d["hostapi"]]["name"], float(d["default_samplerate"]),
                             int(d["max_output_channels"]))
                for i, d in enumerate(devs) if d["max_output_channels"] > 0]
    except Exception:  # no PortAudio / no devices
        return []


def pick_device(devices: list[OutputDevice], default_index: int | None = None) -> int:
    """JACK with 2+ outputs > system default > RuntimeError."""
    for d in devices:
        if d.hostapi == JACK and d.channels >= 2:
            return d.index
    if default_index is not None and default_index >= 0 and any(d.index == default_index for d in devices):
        return default_index
    raise RuntimeError("No audio output device available. Check that PipeWire/JACK or ALSA exposes an output.")


def _default_output_index() -> int | None:
    try:
        d = sd.default.device
        return int(d[1] if hasattr(d, "__getitem__") else d)
    except Exception:
        return None


class AudioStream:
    def __init__(self, engine, device_index=None, samplerate=48000, blocksize=512, scope=None):
        # blocksize 512: 8 voices at high resonance can exceed the 256-frame budget (ruling R9)
        self.engine, self.device_index = engine, device_index
        self.samplerate, self.blocksize, self.scope = samplerate, blocksize, scope
        self.xruns = 0
        self.errors = 0
        self.callbacks = 0
        self.max_late_ms = 0.0
        self._ref = None
        self._done = 0
        self._stream = None
        self.active_device = None

    def callback(self, outdata, frames, time_info, status):
        self.callbacks += 1
        try:
            if status is not None and getattr(status, "output_underflow", False):
                self.xruns += 1
            # shortcut: lateness heuristic assumes the host delivers bursts of <=2 blocks
            # (JACK period <= 2*blocksize); deeper bursts can yield one false xrun each.
            # The underflow flag above is the authoritative signal.
            t = getattr(time_info, "currentTime", None)
            if t is not None:
                nominal = frames / self.samplerate
                if self._ref is None:
                    self._ref = t
                late = t - (self._ref + self._done / self.samplerate)
                if late > nominal * LATE_FACTOR:
                    self.xruns += 1
                    self._ref = t - self._done / self.samplerate
                elif late < 0:
                    self._ref = t - self._done / self.samplerate
                self.max_late_ms = max(self.max_late_ms, late * 1000.0)
            self._done += frames
            left = outdata[:, 0]
            self.engine.render(left)
            np.copyto(outdata[:, 1], left)
            if self.scope is not None:
                self.scope.write(left)
        except Exception:
            self.errors += 1
            outdata.fill(0.0)

    def start(self):
        if self._stream is not None:
            if not self._stream.active:  # restart after stop()
                self._ref, self._done = None, 0
                self._stream.start()
            return
        self._ref, self._done = None, 0
        idx = self.device_index
        if idx is None:
            idx = pick_device(list_output_devices(), _default_output_index())
        self.active_device = idx  # resolved index, so the GUI label never lies about auto-pick
        try:
            self._stream = sd.OutputStream(samplerate=self.samplerate, blocksize=self.blocksize,
                                           device=idx, channels=2, dtype="float32",
                                           latency="low", callback=self.callback)
            self._stream.start()
        except (sd.PortAudioError, ValueError) as e:
            self.close()  # don't leak the PortAudio handle if start() failed
            raise RuntimeError(f"Could not open audio device {idx}: {e}") from None

    def stop(self):
        if self._stream is not None:
            try:
                self._stream.stop()
            except Exception:
                pass

    def close(self):
        s, self._stream = self._stream, None
        if s is not None:
            try:
                s.stop()
                s.close()
            except Exception:
                pass
