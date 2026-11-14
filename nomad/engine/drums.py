"""Drum machine: synthesized one-shots, 16-step sequencer and mix.

Everything here belongs to ``nomad.engine`` and therefore imports nothing from
the GUI or the MIDI layer. The GUI writes state (floats, a ``uint8`` step
matrix and array references) and the audio thread reads it; the stores are
atomic and the worst case is one control block of latency, so no queue is
needed for commands whose order does not matter.

One-shots are pre-rendered: changing ``tune`` or ``decay`` rebuilds that
instrument's array on the caller thread and swaps the reference, keeping the
audio callback allocation-free.
"""

from __future__ import annotations

import math
import time

import numpy as np
from scipy.signal import butter, lfilter

from nomad.engine.params import ParamSpec

__all__ = [
    "DRUM_REGISTRY",
    "INSTRUMENTS",
    "STEPS",
    "DrumMachine",
    "render_one_shot",
]

INSTRUMENTS: tuple[str, ...] = ("kick", "snare", "clap", "tom", "chat", "ohat")
STEPS = 16

_SEED = {"kick": 1, "snare": 2, "clap": 3, "tom": 4, "chat": 5, "ohat": 6}
_LEVEL = {"kick": 0.6, "snare": 0.5, "clap": 0.45, "tom": 0.5, "chat": 0.35, "ohat": 0.3}
_MAX_SECONDS = 1.5
_TAIL_FADE_S = 0.01
_PEAK = 0.9


def _build_registry() -> dict[str, ParamSpec]:
    reg: dict[str, ParamSpec] = {}
    for track in INSTRUMENTS:
        reg[f"{track}_tune"] = ParamSpec(f"{track}_tune", 0.0, -1.0, 1.0, "lin")
        reg[f"{track}_decay"] = ParamSpec(f"{track}_decay", 0.5, 0.0, 1.0, "lin")
        reg[f"{track}_level"] = ParamSpec(f"{track}_level", _LEVEL[track], 0.0, 1.0, "lin")
    reg["bpm"] = ParamSpec("bpm", 110.0, 60.0, 180.0, "lin", "BPM")
    reg["swing"] = ParamSpec("swing", 0.0, 0.0, 0.6, "lin")
    return reg


DRUM_REGISTRY: dict[str, ParamSpec] = _build_registry()


def _clamp(spec: ParamSpec, value: float) -> float:
    v = float(value)
    if not math.isfinite(v):
        return spec.default
    return min(max(v, spec.lo), spec.hi)


def _length(tau: float, sr: float) -> int:
    n = int(10.0 * tau * sr) + int(0.02 * sr)
    return max(int(0.04 * sr), min(int(_MAX_SECONDS * sr), n))


def _env(t: np.ndarray, tau: float) -> np.ndarray:
    return np.exp(-t / max(tau, 1e-4))


def _noise(rng: np.random.Generator, n: int, sr: float, cutoff: float) -> np.ndarray:
    """White noise through a first-order high-pass at ``cutoff`` Hz."""
    x = rng.standard_normal(n)
    a = 1.0 / (1.0 + 2.0 * np.pi * min(max(cutoff, 100.0), 0.45 * sr) / sr)
    return lfilter([a, -a], [1.0, -a], x)


def _band_noise(rng: np.random.Generator, n: int, sr: float, centre: float,
                octaves: float = 1.5) -> np.ndarray:
    """White noise through a wide band-pass centred on ``centre`` Hz."""
    low = max(centre * 2.0 ** -octaves, 30.0)
    high = min(centre * 2.0 ** octaves, 0.45 * sr)
    if high <= low:
        return np.zeros(n)
    b, a = butter(1, [low / (sr / 2.0), high / (sr / 2.0)], btype="bandpass")
    return lfilter(b, a, rng.standard_normal(n))


def _tone(freq: np.ndarray, sr: float) -> np.ndarray:
    return np.sin(2.0 * np.pi * np.cumsum(freq) / sr)


def _kick(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    base = 55.0 * 2.0 ** (tune * 0.5)
    freq = base * (1.0 + 1.6 * np.exp(-t / 0.030))
    body = _tone(freq, sr) * _env(t, 0.08 + 0.30 * decay)
    click = _noise(rng, len(t), sr, 2000.0) * _env(t, 0.002) * 0.3
    return body + click


def _snare(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    shift = 2.0 ** (tune * 0.3)
    tone = _tone(np.full(len(t), 180.0 * shift), sr) + 0.7 * _tone(np.full(len(t), 330.0 * shift), sr)
    body = tone * _env(t, 0.05 + 0.10 * decay)
    noise = _noise(rng, len(t), sr, 1200.0 * 2.0 ** (tune * 1.5)) * _env(t, 0.05 + 0.18 * decay)
    return 0.8 * body + noise


def _clap(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    spread = 2.0 ** (tune * 0.25)
    bursts = np.zeros_like(t)
    for i in range(4):
        centre = i * 0.010 / spread
        bursts += np.exp(-(((t - centre) / 0.0035) ** 2))
    tail = 0.6 * _env(t, 0.04 + 0.12 * decay)
    return _band_noise(rng, len(t), sr, 1000.0 * 2.0 ** (tune * 1.5)) * (bursts + tail)


def _tom(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    base = 100.0 * 2.0 ** (tune * 0.5)
    freq = base * (1.0 + 0.6 * np.exp(-t / 0.060))
    return _tone(freq, sr) * _env(t, 0.12 + 0.30 * decay)


def _hat(t: np.ndarray, sr: float, tune: float, decay: float, rng, tau: float) -> np.ndarray:
    return _band_noise(rng, len(t), sr, 6500.0 * 2.0 ** (tune * 1.5)) * _env(t, tau)


def render_one_shot(kind: str, sr: float = 48000.0, tune: float = 0.0,
                    decay: float = 0.5) -> np.ndarray:
    """Mono float32 one-shot for ``kind``, normalized to 0.9 and deterministic."""
    if kind not in INSTRUMENTS:
        raise ValueError(f"unknown drum instrument: {kind!r}")
    sr = float(sr)
    tune = _clamp(DRUM_REGISTRY[f"{kind}_tune"], tune)
    decay = _clamp(DRUM_REGISTRY[f"{kind}_decay"], decay)
    rng = np.random.default_rng(_SEED[kind])

    tau = _TAU[kind](decay)
    n = _length(tau, sr)
    t = np.arange(n) / sr

    if kind == "kick":
        y = _kick(t, sr, tune, decay, rng)
    elif kind == "snare":
        y = _snare(t, sr, tune, decay, rng)
    elif kind == "clap":
        y = _clap(t, sr, tune, decay, rng)
    elif kind == "tom":
        y = _tom(t, sr, tune, decay, rng)
    else:
        y = _hat(t, sr, tune, decay, rng, tau)

    y = np.asarray(y, dtype=np.float64)
    fade = min(int(_TAIL_FADE_S * sr), n)
    if fade > 1:
        y[-fade:] *= np.linspace(1.0, 0.0, fade)
    peak = float(np.abs(y).max())
    if peak > 0.0:
        y *= _PEAK / peak
    return y.astype(np.float32)


_TAU = {
    "kick": lambda decay: 0.08 + 0.30 * decay,
    "snare": lambda decay: 0.05 + 0.18 * decay,
    "clap": lambda decay: 0.04 + 0.12 * decay,
    "tom": lambda decay: 0.12 + 0.30 * decay,
    "chat": lambda decay: 0.02 + 0.05 * decay,
    "ohat": lambda decay: 0.12 + 0.20 * decay,
}

ACCENT_GAIN = 1.0
NORMAL_GAIN = 0.6
FADE_MS = 5.0
REGEN_COOLDOWN_S = 0.06
_HARD_LEVEL = (0, 1, 2)


class DrumMachine:
    """Six synthesized drums, a 16-step sequencer and an allocation-free mix.

    Threading: the GUI writes ``values``/``steps`` and swaps ``_bank`` entries;
    all of them are single atomic stores under the GIL, so the audio thread may
    read them without a lock (worst case: one control block of latency).
    ``render`` is the audio thread: it never allocates and never raises.
    """

    def __init__(self, sample_rate: float = 48000.0) -> None:
        self.sr = float(sample_rate)
        self.values: dict[str, float] = {n: s.default for n, s in DRUM_REGISTRY.items()}
        self.steps = np.zeros((len(INSTRUMENTS), STEPS), dtype=np.uint8)
        self.playing = False
        self.position = 0
        self.errors = 0
        self._bank: list[np.ndarray] = [
            render_one_shot(t, self.sr, self.values[f"{t}_tune"], self.values[f"{t}_decay"])
            for t in INSTRUMENTS
        ]
        self._active: list[list] = [[None, 0, 0.0] for _ in INSTRUMENTS]
        self._phase = 0.0
        self._pending_start = False
        self._fade_len = max(1, int(FADE_MS / 1000.0 * self.sr))
        self._fade_left = 0
        self._dirty: set[int] = set()
        self._last_regen = 0.0
        cap = 2048
        self._mix = np.zeros(cap)
        self._tmp = np.zeros(cap)
        self._gain = np.zeros(cap)
        self._ramp = np.arange(cap, dtype=np.float64)
        self._default_pattern()

    # ---- caller thread -----------------------------------------------------
    def _default_pattern(self) -> None:
        self.clear_pattern()
        for step in (0, 4, 8, 12):
            self.steps[0, step] = 1
        self.steps[0, 0] = 2
        for step in (4, 12):
            self.steps[1, step] = 1
        for step in range(0, STEPS, 2):
            self.steps[4, step] = 1

    def clear_pattern(self) -> None:
        self.steps.fill(0)

    def set_step(self, track: str, step: int, level: int) -> None:
        i = INSTRUMENTS.index(track)
        level = min(max(int(level), 0), 2)
        self.steps[i, step] = level

    def set_value(self, name: str, value: float) -> None:
        spec = DRUM_REGISTRY[name]
        v = _clamp(spec, value)
        self.values[name] = v
        track, _, field = name.partition("_")
        if field in ("tune", "decay"):
            i = INSTRUMENTS.index(track)
            now = time.perf_counter()
            if now - self._last_regen < REGEN_COOLDOWN_S:
                self._dirty.add(i)
                return
            self._regen(i)
            self._last_regen = now

    def _regen(self, i: int) -> None:
        track = INSTRUMENTS[i]
        self._bank[i] = render_one_shot(track, self.sr, self.values[f"{track}_tune"],
                                        self.values[f"{track}_decay"])

    def flush(self) -> bool:
        """Apply tone changes deferred by the throttle. Caller thread (GUI poll)."""
        if not self._dirty:
            return False
        for i in sorted(self._dirty):
            self._regen(i)
        self._dirty.clear()
        self._last_regen = time.perf_counter()
        return True

    def trigger(self, track: str, velocity: float = 1.0) -> None:
        i = INSTRUMENTS.index(track)
        v = float(velocity)
        v = v if math.isfinite(v) else 1.0
        state = self._active[i]
        state[0] = self._bank[i]
        state[1] = 0
        state[2] = min(max(v, 0.0), 1.0) * self.values[f"{track}_level"]

    def start(self) -> None:
        if self.playing:
            return
        self.playing = True
        self.position = 0
        self._phase = 0.0
        self._pending_start = True

    def stop(self) -> None:
        self.playing = False
        self.position = 0
        self._phase = 0.0
        self._pending_start = False

    def toggle(self) -> None:
        self.stop() if self.playing else self.start()

    def stop_all(self) -> None:
        self.stop()
        self._fade_left = self._fade_len

    # ---- audio thread ------------------------------------------------------
    def _duration(self, step: int) -> float:
        base = self.sr * 60.0 / self.values["bpm"] / 4.0
        swing = self.values["swing"]
        return base * (1.0 + swing) if step % 2 == 0 else base * (1.0 - swing)

    def _fire(self, step: int) -> None:
        for i, track in enumerate(INSTRUMENTS):
            level = int(self.steps[i, step])
            if level:
                self.trigger(track, ACCENT_GAIN if level == _HARD_LEVEL[2] else NORMAL_GAIN)

    def _ensure(self, n: int) -> None:
        if n <= self._mix.shape[0]:
            return
        self._mix = np.zeros(n)
        self._tmp = np.zeros(n)
        self._gain = np.zeros(n)
        self._ramp = np.arange(n, dtype=np.float64)

    def _render_chunk(self, out: np.ndarray, start: int, chunk: int) -> None:
        mix = self._mix[:chunk]
        mix[:] = 0.0
        for state in self._active:
            arr, off, gain = state
            if arr is None or off >= arr.shape[0] or gain == 0.0:
                continue
            seg = min(chunk, arr.shape[0] - off)
            tmp = self._tmp[:seg]
            np.multiply(arr[off:off + seg], gain, out=tmp)
            mix[:seg] += tmp
            state[1] = off + seg

        if self._fade_left > 0:
            g0 = min(1.0, self._fade_left / self._fade_len)
            left = max(0, self._fade_left - chunk)
            g1 = left / self._fade_len
            g = self._gain[:chunk]
            np.multiply(self._ramp[:chunk], (g1 - g0) / chunk, out=g)
            g += g0
            np.multiply(mix, g, out=mix)
            self._fade_left = left
            if left == 0:
                self._clear_active()

        np.add(out[start:start + chunk], mix, out=out[start:start + chunk])

    def _clear_active(self) -> None:
        for state in self._active:
            state[0] = None
            state[1] = 0
            state[2] = 0.0

    def render(self, out: np.ndarray, n: int) -> None:
        n = min(int(n), out.shape[0])
        if n <= 0:
            return
        if not self.playing and self._fade_left == 0:
            if all(s[0] is None or s[1] >= s[0].shape[0] for s in self._active):
                return
        self._ensure(n)
        i = 0
        while i < n:
            if self._pending_start:
                self._pending_start = False
                self._fire(0)
            if self.playing:
                dur = self._duration(self.position)
                remain = (1.0 - self._phase) * dur
                chunk = min(n - i, max(1, int(math.ceil(remain))))
            else:
                chunk = n - i
                if self._fade_left > 0:
                    chunk = min(chunk, self._fade_left)
            self._render_chunk(out, i, chunk)
            i += chunk
            if self.playing:
                self._phase += chunk / dur
                if self._phase >= 1.0 - 1e-9:
                    self._phase = max(0.0, self._phase - 1.0)
                    self.position = (self.position + 1) % STEPS
                    self._fire(self.position)
