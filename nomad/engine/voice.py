"""One synth voice: two oscillators, ladder filter, two ADSRs.

``render`` is called once per control block (<= 256 frames, R6). Oscillators
and envelopes run over the whole block; only the filter is sub-divided into
grains (16 samples when k > 3.2, else 64) with the cutoff recomputed -- and
smoothed in the log domain -- per grain, so switching coefficients never steps.

Units of ``mods`` (a SHARED dict from ModMatrix.resolve plus ``bend``; read
immediately, never stored): ``pitch``/``pitch1``/``pitch2``/``bend`` are
semitones; ``cutoff`` is x2 octaves; ``pw1``/``pw2`` add x0.45 to the pulse
width; ``amp`` is added to a unit gain.

Steal: ``begin_steal`` fades the sounding note out linearly over STEAL_FADE_SEC
(5 ms), then resets the amp envelope to 0 and retriggers the new note while the
output gain is exactly 0, so the switch is sample-continuous. The retrigger
lands at the end of the block in which the fade completes (<= one block late).
"""

from __future__ import annotations

import math

import numpy as np

from nomad.engine.envelope import Envelope
from nomad.engine.filter import K_MAX, Ladder4Pole
from nomad.engine.oscillator import MAX_INC, Oscillator

__all__ = ["Voice", "STEAL_FADE_SEC"]

STEAL_FADE_SEC = 0.005
_CUTOFF_TAU = 0.003  # s, smoothing of the log-cutoff across grains
_LOG_LO, _LOG_HI = math.log2(20.0), math.log2(18000.0)
_BLOCK = 256


def _f(x: float, default: float = 0.0) -> float:
    x = float(x)
    return x if math.isfinite(x) else default


class Voice:
    def __init__(self, sample_rate: float, slot: int) -> None:
        self.sr = float(sample_rate)
        self.slot = slot
        self.note = -1
        self.age = 0
        self.velocity = 0.0
        self.env2_level = 0.0
        self.osc1 = Oscillator(self.sr)
        self.osc2 = Oscillator(self.sr)
        self.filter = Ladder4Pole(self.sr)
        self.amp_env = Envelope(self.sr)
        self.filt_env = Envelope(self.sr)
        self._pitch_note = 60  # key the oscillators follow (old note during a steal)
        self._cur = 60.0  # pitch actually sounding, in semitones: glides towards ``_pitch_note``
        self._lc = None  # smoothed log2 cutoff; None = initialise on next grain
        self._fade_left = 0
        self._pending: tuple[int, float] | None = None
        self._alloc(_BLOCK)

    def _alloc(self, n: int) -> None:
        self._n = n
        self._ramp = np.arange(n, dtype=np.float64)
        self._mix = np.empty(n)
        self._tmp = np.empty(n)
        self._master = np.empty(n)
        self._aenv = np.empty(n)
        self._fenv = np.empty(n)

    def is_active(self) -> bool:
        return self._pending is not None or self._fade_left > 0 or not self.amp_env.is_idle()

    def _trigger(self, note: int, velocity: float, start: float | None = None) -> None:
        if self.amp_env.is_idle():  # fresh start: deterministic phase and clean filter
            self.osc1.phase = self.osc2.phase = 0.0
            self.filter.reset()
            self._lc = None
            self._cur = float(note if start is None else start)  # a steal keeps ``_cur``: it glides on from there
        self.note = self._pitch_note = int(note)
        self.velocity = min(max(_f(velocity), 0.0), 1.0)
        self.amp_env.note_on()
        self.filt_env.note_on()

    def note_on(self, note: int, velocity: float, start: float | None = None) -> None:
        self._pending, self._fade_left = None, 0
        self._trigger(note, velocity, start)

    def set_target(self, note: int) -> None:
        """Legato: glide to ``note`` without touching the envelopes."""
        self.note = self._pitch_note = int(note)

    def note_off(self) -> None:
        if self._pending is not None:  # cancel the retrigger; the fade ends in silence
            self._pending = None
            self._fade_left = max(self._fade_left, 1)
        self.amp_env.note_off()
        self.filt_env.note_off()

    def begin_steal(self, note: int, velocity: float) -> None:
        if not self.is_active():
            return self.note_on(note, velocity)
        self.note = int(note)  # so the manager's note_off lookup finds the new key
        self._pending = (int(note), velocity)
        # A second steal during a running fade only retargets the pending note:
        # restarting the fade would jump the gain back to 1 (click).
        if self._fade_left == 0:
            self._fade_left = max(1, int(STEAL_FADE_SEC * self.sr))

    def _freq(self, p, m, n: int, mode_key, hz_key, coarse_key, fine_key, mod_key) -> float:
        semis = _f(m[mod_key]) + _f(m["pitch"])
        if _f(p[mode_key]) >= 0.5:  # free: absolute Hz, ignores key and bend
            base, semis = _f(p[hz_key], 220.0), semis + _f(p[fine_key]) / 100.0
        else:
            semis += self._cur - 69 + _f(p[coarse_key]) + _f(p[fine_key]) / 100.0 + _f(m["bend"])
            base = 440.0
        return base * 2.0 ** (min(max(semis, -200.0), 200.0) / 12.0)

    def render(self, out: np.ndarray, p: dict, mods: dict) -> None:
        n = out.shape[0]
        if n == 0 or not self.is_active():
            return
        if n > self._n:
            self._alloc(n)
        sr = self.sr
        a_env, f_env = self._aenv[:n], self._fenv[:n]
        mix, tmp = self._mix[:n], self._tmp[:n]
        self.amp_env.set_params(p["amp_attack"] / 1e3, p["amp_decay"] / 1e3, p["amp_sustain"], p["amp_release"] / 1e3)
        self.filt_env.set_params(p["filt_attack"] / 1e3, p["filt_decay"] / 1e3, p["filt_sustain"], p["filt_release"] / 1e3)
        self.amp_env.process(a_env)
        self.filt_env.process(f_env)
        self.env2_level = float(f_env[n - 1])

        # portamento (shortcut: one frequency per block, so glides under ~20 ms sound stepped)
        glide = _f(p.get("glide", 0.0))
        d = self._pitch_note - self._cur
        if glide <= 0.0 or abs(d) < 1e-3:
            self._cur = float(self._pitch_note)
        else:
            self._cur += d * (1.0 - math.exp(-n / (sr * glide / 3000.0)))  # RC curve: glide ms ~ 3 time constants

        # oscillators: whole block each
        mix[:] = 0.0
        f1 = self._freq(p, mods, n, "osc1_mode", "osc1_hz", "osc1_coarse", "osc1_fine", "pitch1")
        f2 = self._freq(p, mods, n, "osc2_mode", "osc2_hz", "osc2_coarse", "osc2_fine", "pitch2")
        o1, o2 = self.osc1, self.osc2
        o1.wave, o2.wave = int(_f(p["osc1_wave"], 2)), int(_f(p["osc2_wave"], 2))
        sync = _f(p["osc2_sync"]) >= 0.5
        if sync:  # osc1's phase for these samples, same ramp the oscillator will use
            inc = min(abs(f1), MAX_INC * sr) / sr
            master = self._master[:n]
            np.multiply(self._ramp[:n], inc, out=master)
            master += o1.phase
            master %= 1.0
        tmp[:] = 0.0
        o1.render_into(tmp, f1, _f(p["osc1_pw"], 0.5) + 0.45 * _f(mods["pw1"]))
        mix += tmp * _f(p["osc1_level"])
        tmp[:] = 0.0
        o2.render_into(tmp, f2, _f(p["osc2_pw"], 0.5) + 0.45 * _f(mods["pw2"]), master if sync else None)
        mix += tmp * _f(p["osc2_level"])

        # filter: grains, cutoff recomputed and smoothed per grain
        reso = min(max(_f(p["filt_reso"]), 0.0), 1.0)
        grain = 16 if reso * K_MAX > 3.2 else 64
        base = math.log2(min(max(_f(p["filt_cutoff"], 1200.0), 20.0), 18000.0))
        amt = 5.0 * _f(p["filt_env_amt"])
        mc = 2.0 * _f(mods["cutoff"])
        coef = 1.0 - math.exp(-grain / (_CUTOFF_TAU * sr))
        lc = self._lc
        for s in range(0, n, grain):
            e = min(n, s + grain)
            target = min(max(base + amt * float(f_env[s]) + mc, _LOG_LO), _LOG_HI)
            lc = target if lc is None else lc + coef * (target - lc)
            self.filter.set(2.0**lc, reso)
            self.filter.process(mix[s:e])
        self._lc = lc

        # amp: envelope, velocity, mod, steal fade -- on the whole block
        mix *= a_env
        mix *= self.velocity * max(0.0, 1.0 + _f(mods["amp"]))
        if self._fade_left > 0:
            total = max(1, int(STEAL_FADE_SEC * sr))
            g = tmp
            np.subtract(self._fade_left, self._ramp[:n], out=g)
            g *= 1.0 / total
            np.clip(g, 0.0, 1.0, out=g)
            mix *= g
            self._fade_left = max(0, self._fade_left - n)
            if self._fade_left == 0:
                self.amp_env.value = 0.0  # output gain is 0 here: no jump
                pend, self._pending = self._pending, None
                if pend is not None:
                    self._trigger(*pend)
                else:
                    self.amp_env.note_off()
        np.nan_to_num(mix, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        out += mix
