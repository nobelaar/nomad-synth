"""Declarative parameter registry and value smoothing.

Every user-facing parameter is described by a frozen :class:`ParamSpec`
with its range, default, curve and display unit. The GUI and the MIDI
layer write raw values; the engine clamps them, maps them through their
curve and smooths them per grain so that no control jump produces a
click.

This module belongs to ``nomad.engine`` and therefore imports nothing
from the GUI, the MIDI layer or the audio backend.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

__all__ = [
    "ParamSpec",
    "REGISTRY",
    "Smoother",
    "clamp",
    "from_norm",
    "to_norm",
]

# Supported transfer curves between raw values and the 0..1 knob domain.
CURVES = ("lin", "exp")


@dataclass(frozen=True, slots=True)
class ParamSpec:
    """Static description of one parameter.

    ``curve`` is ``"lin"`` for a linear mapping between ``lo`` and ``hi``
    or ``"exp"`` for a logarithmic one (both bounds must be positive in
    that case). ``unit`` is only used for display; ``smooth_ms`` is the
    one-pole time constant of the per-parameter smoother.
    """

    name: str
    default: float
    lo: float
    hi: float
    curve: str
    unit: str = ""
    smooth_ms: float = 15.0

    def __post_init__(self) -> None:
        if self.curve not in CURVES:
            raise ValueError(f"{self.name}: unknown curve {self.curve!r}")
        if not self.lo < self.hi:
            raise ValueError(f"{self.name}: lo must be below hi")
        if self.curve == "exp" and self.lo <= 0.0:
            raise ValueError(f"{self.name}: exp curve needs lo > 0")


def _build_registry() -> dict[str, ParamSpec]:
    """Build the parameter registry in the canonical UI order."""
    specs: list[ParamSpec] = [
        ParamSpec("polyphony", 8.0, 1.0, 8.0, "lin", "voices"),
        ParamSpec("glide", 0.0, 0.0, 1000.0, "lin", "ms"),
        ParamSpec("master_volume", 0.7, 0.0, 1.0, "lin"),
    ]
    for n in (1, 2):
        specs += [
            ParamSpec(f"osc{n}_wave", 2.0, 0.0, 3.0, "lin"),
            ParamSpec(f"osc{n}_mode", 0.0, 0.0, 1.0, "lin"),
            ParamSpec(f"osc{n}_coarse", 0.0, -24.0, 24.0, "lin", "st"),
            ParamSpec(f"osc{n}_hz", 220.0, 20.0, 8000.0, "exp", "Hz"),
            ParamSpec(f"osc{n}_fine", 0.0, -50.0, 50.0, "lin", "ct"),
            ParamSpec(f"osc{n}_pw", 0.5, 0.05, 0.95, "lin"),
            ParamSpec(f"osc{n}_level", 0.5, 0.0, 1.0, "lin"),
            ParamSpec(f"osc{n}_sync", 0.0, 0.0, 1.0, "lin"),
        ]
    specs += [
        ParamSpec("filt_cutoff", 1200.0, 20.0, 18000.0, "exp", "Hz"),
        ParamSpec("filt_reso", 0.3, 0.0, 1.0, "lin"),
        ParamSpec("filt_env_amt", 0.0, -1.0, 1.0, "lin"),
    ]
    specs += [
        ParamSpec("amp_attack", 5.0, 1.0, 5000.0, "exp", "ms"),
        ParamSpec("amp_decay", 300.0, 1.0, 5000.0, "exp", "ms"),
        ParamSpec("amp_sustain", 0.8, 0.0, 1.0, "lin"),
        ParamSpec("amp_release", 400.0, 5.0, 8000.0, "exp", "ms"),
    ]
    specs += [
        ParamSpec("filt_attack", 5.0, 1.0, 5000.0, "exp", "ms"),
        ParamSpec("filt_decay", 250.0, 1.0, 5000.0, "exp", "ms"),
        ParamSpec("filt_sustain", 0.3, 0.0, 1.0, "lin"),
        ParamSpec("filt_release", 400.0, 5.0, 8000.0, "exp", "ms"),
    ]
    for n in (1, 2):
        specs += [
            ParamSpec(f"lfo{n}_rate", 1.0, 0.02, 40.0, "exp", "Hz"),
            ParamSpec(f"lfo{n}_wave", 0.0, 0.0, 4.0, "lin"),
            ParamSpec(f"lfo{n}_depth", 0.0, 0.0, 1.0, "lin"),
            ParamSpec(f"lfo{n}_phase", 0.0, 0.0, 1.0, "lin"),
        ]
    for i in range(8):
        specs += [
            ParamSpec(f"mod{i}_src", 0.0, 0.0, 5.0, "lin"),
            ParamSpec(f"mod{i}_dst", 0.0, 0.0, 7.0, "lin"),
            ParamSpec(f"mod{i}_amt", 0.0, -1.0, 1.0, "lin"),
        ]
    specs.append(ParamSpec("bend_range", 2.0, 1.0, 12.0, "lin", "st"))
    return {spec.name: spec for spec in specs}


REGISTRY: dict[str, ParamSpec] = _build_registry()


def clamp(name: str, value: float) -> float:
    """Return ``value`` limited to the range of parameter ``name``.

    Raises :class:`KeyError` for unknown parameter names. Non-finite
    inputs (NaN, infinities) return the parameter default instead of
    poisoning the smoothing chain.
    """
    spec = REGISTRY[name]
    v = float(value)
    if not math.isfinite(v):
        return spec.default
    return min(max(v, spec.lo), spec.hi)


def to_norm(name: str, value: float) -> float:
    """Map a raw ``value`` of ``name`` onto its 0..1 knob position."""
    spec = REGISTRY[name]
    v = clamp(name, value)
    if spec.curve == "exp":
        return math.log(v / spec.lo) / math.log(spec.hi / spec.lo)
    return (v - spec.lo) / (spec.hi - spec.lo)


def from_norm(name: str, norm: float) -> float:
    """Map a 0..1 knob position back to a raw value of ``name``."""
    spec = REGISTRY[name]
    n = min(max(float(norm), 0.0), 1.0)
    if spec.curve == "exp":
        return spec.lo * (spec.hi / spec.lo) ** n
    return spec.lo + n * (spec.hi - spec.lo)


class Smoother:
    """One-pole exponential smoother for a single parameter.

    ``time_ms`` is the time constant of the one-pole filter, so the value
    settles to within 0.1 % of the target after roughly 6.9 times
    ``time_ms``. Every :meth:`advance` call moves the value towards
    :attr:`target` with the coefficient ``1 - exp(-frames / tau)``, where
    ``tau = time_ms/1000 * sample_rate``, so the target is never overshot.
    """

    def __init__(self, target: float, sample_rate: float, time_ms: float) -> None:
        self.target = float(target)
        self.value = float(target)
        self.sample_rate = float(sample_rate)
        self.time_ms = float(time_ms)

    @property
    def _tau(self) -> float:
        """Time constant in samples, derived from the current settings."""
        return max(self.time_ms / 1000.0 * self.sample_rate, 1e-9)

    def set(self, target: float) -> None:
        """Move the target; :attr:`value` follows on the next advances."""
        self.target = float(target)

    def advance(self, frames: int) -> float:
        """Advance ``frames`` samples towards the target and return the value."""
        if frames <= 0:
            return self.value
        coefficient = 1.0 - math.exp(-frames / self._tau)
        self.value += coefficient * (self.target - self.value)
        return self.value
