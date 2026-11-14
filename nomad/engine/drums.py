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

import numpy as np

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


def _noise(rng: np.random.Generator, n: int) -> np.ndarray:
    """High-passed white noise: the first difference is a 6 dB/oct HP."""
    return np.diff(rng.standard_normal(n), prepend=0.0)


def _tone(freq: np.ndarray, sr: float) -> np.ndarray:
    return np.sin(2.0 * np.pi * np.cumsum(freq) / sr)


def _kick(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    base = 55.0 * 2.0 ** (tune * 0.5)
    freq = base * (1.0 + 1.6 * np.exp(-t / 0.030))
    body = _tone(freq, sr) * _env(t, 0.08 + 0.30 * decay)
    click = _noise(rng, len(t)) * _env(t, 0.002) * 0.3
    return body + click


def _snare(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    shift = 2.0 ** (tune * 0.3)
    tone = _tone(np.full(len(t), 180.0 * shift), sr) + 0.7 * _tone(np.full(len(t), 330.0 * shift), sr)
    body = tone * _env(t, 0.05 + 0.10 * decay)
    noise = _noise(rng, len(t)) * _env(t, 0.05 + 0.18 * decay)
    return 0.8 * body + noise


def _clap(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    spread = 2.0 ** (tune * 0.25)
    bursts = np.zeros_like(t)
    for i in range(4):
        centre = i * 0.010 / spread
        bursts += np.exp(-(((t - centre) / 0.0035) ** 2))
    tail = 0.6 * _env(t, 0.04 + 0.12 * decay)
    return _noise(rng, len(t)) * (bursts + tail)


def _tom(t: np.ndarray, sr: float, tune: float, decay: float, rng) -> np.ndarray:
    base = 100.0 * 2.0 ** (tune * 0.5)
    freq = base * (1.0 + 0.6 * np.exp(-t / 0.060))
    return _tone(freq, sr) * _env(t, 0.12 + 0.30 * decay)


def _hat(t: np.ndarray, sr: float, tune: float, decay: float, rng, tau: float) -> np.ndarray:
    y = _noise(rng, len(t)) * _env(t, tau)
    return y + 0.15 * _tone(np.full(len(t), 7000.0 * 2.0 ** (tune * 0.5)), sr) * _env(t, tau)


def render_one_shot(kind: str, sr: float = 48000.0, tune: float = 0.0,
                    decay: float = 0.5) -> np.ndarray:
    """Mono float32 one-shot for ``kind``, normalized to 0.9 and deterministic."""
    if kind not in INSTRUMENTS:
        raise ValueError(f"unknown drum instrument: {kind!r}")
    sr = float(sr)
    tune = _clamp(DRUM_REGISTRY[f"{kind}_tune"], tune)
    decay = _clamp(DRUM_REGISTRY[f"{kind}_decay"], decay)
    rng = np.random.default_rng(_SEED[kind])

    tau = {"kick": 0.08 + 0.30 * decay, "snare": 0.05 + 0.18 * decay,
           "clap": 0.04 + 0.12 * decay, "tom": 0.12 + 0.30 * decay,
           "chat": 0.02 + 0.05 * decay, "ohat": 0.12 + 0.20 * decay}[kind]
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
