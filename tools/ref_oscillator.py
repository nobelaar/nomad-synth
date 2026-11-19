"""Oscillator reference scenarios (free running) for the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.oscillator import MAX_INC, Oscillator
from tools.refio import BlobWriter

SR = 48000.0
FREE_BLOCKS = [256, 100, 1, 17]
WAVES = [0, 1, 2, 3]
FREQS = [55.0, 440.0, 5200.0, 12000.0, 21000.0, 0.0]
PWS = [0.5, 0.2]
EDGES = {  # label: (wave, freq, pw); square at 440 Hz unless the edge is the frequency
    "edge/nan_freq": (3, float("nan"), 0.5),
    "edge/neg_freq": (3, -440.0, 0.5),
    "edge/pw_nan": (3, 440.0, float("nan")),
    "edge/pw_0": (3, 440.0, 0.0),
    "edge/pw_1": (3, 440.0, 1.0),
}


def render_blocks(wave, freq, pw, blocks, sync=None):
    """Render contiguous blocks (each from zeros) and return (samples, end phase)."""
    osc = Oscillator(SR)
    osc.wave = wave
    out = []
    for n in blocks:
        buf = np.zeros(n)
        osc.render_into(buf, freq, pw)
        out.append(buf)
    return np.concatenate(out), osc.phase


def write(out_dir: Path) -> None:
    w = BlobWriter("oscillator")
    for wave in WAVES:
        for freq in FREQS:
            for pw in PWS:
                data, ph = render_blocks(wave, freq, pw, FREE_BLOCKS)
                w.add(f"free/{wave}/{freq}/{pw}", data, wave=wave, freq=freq, pw=pw,
                      blocks=FREE_BLOCKS, phase_end=ph)
    for label, (wave, freq, pw) in EDGES.items():
        data, ph = render_blocks(wave, freq, pw, [256])
        w.add(label, data, wave=wave, freq=freq, pw=pw, blocks=[256], phase_end=ph)
    w.write(out_dir)
    write_sync(out_dir)


SYNC_BLOCKS = [64, 64, 100, 7, 256]
SYNC_RATIOS = [0.5, 1.37, 2.0, 3.9]
SLAVE_HZ = 300.0


def render_synced(wave, slave_hz, master_hz, blocks, synced):
    """Slave with hard sync to a master oscillator, master phase built like Voice.render."""
    master = Oscillator(SR)
    slave = Oscillator(SR)
    slave.wave = wave
    minc = min(abs(master_hz), MAX_INC * SR) / SR
    out = []
    for n, on in zip(blocks, synced):
        mp = (np.arange(n, dtype=np.float64) * minc + master.phase) % 1.0
        master.render_into(np.zeros(n), master_hz)  # advances the master phase
        buf = np.zeros(n)
        slave.render_into(buf, slave_hz, 0.5, mp if on else None)
        out.append(buf)
    return np.concatenate(out), slave.phase


def write_sync(out_dir: Path) -> None:
    w = BlobWriter("oscillator_sync")

    def add(label, wave, slave_hz, master_hz, blocks, synced):
        data, ph = render_synced(wave, slave_hz, master_hz, blocks, synced)
        w.add(label, data, wave=wave, slave_hz=slave_hz, master_hz=master_hz,
              blocks=blocks, synced=synced, phase_end=ph)

    for wave in WAVES:
        for ratio in SYNC_RATIOS:
            add(f"sync/{wave}/{ratio}", wave, SLAVE_HZ, SLAVE_HZ * ratio, SYNC_BLOCKS,
                [True] * len(SYNC_BLOCKS))
    # 750 Hz at 48 kHz is exactly one master cycle per 64-sample block: every wrap
    # lands on a block boundary.
    add("sync/boundary", 2, SLAVE_HZ, 750.0, [64] * 8, [True] * 8)
    # A free block in between must forget the master's last sample.
    add("sync/after_free", 2, SLAVE_HZ, 750.0, [64] * 3, [True, False, True])
    w.write(out_dir)
