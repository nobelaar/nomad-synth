"""Sequencer playback traces: note events of one SynthInstrument's sequencer."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.instrument import SynthInstrument
from tools.refio import BlobWriter

SR = 48000
BEAT = 24000  # frames per beat at 120 bpm


def pattern(notes, vel=100):
    return [[k, 1, n, vel] for k, n in enumerate(notes)]


def scenario(frames, steps=(), extra=(), events=(), **cfg):
    base = dict(bpm=120.0, division=0.25, length=16, swing=0.0, gate=0.5, enabled=True)
    base.update(cfg)
    return dict(base, frames=frames, steps=list(steps), extra=list(extra),
                events=[[0, "start"]] + list(events))


SIX = [60, 62, 64, 67, 69, 72]
CHORDS = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]]
PLAY = {
    "seq/mono_16": scenario(8 * BEAT, pattern([SIX[k % 6] for k in range(16)])),
    "seq/chord_bar": scenario(16 * BEAT, pattern([c[0] for c in CHORDS]),
                              [[k, n] for k, c in enumerate(CHORDS) for n in c[1:]],
                              division=4.0, length=4, gate=1.0),
    "seq/gate_short": scenario(4 * BEAT, pattern([60, 64] * 4), gate=0.1),
    "seq/swing": scenario(4 * BEAT, [[k, 1, 60 + k, 100] for k in range(0, 16, 2)]
                          + [[k, 1, 48 + k, 80] for k in range(1, 16, 2)], swing=0.4),
    "seq/disabled_then_enabled": scenario(4 * BEAT, pattern([60] * 16), enabled=False,
                                          events=[[50000, "enable", True], [80000, "enable", False],
                                                  [85000, "enable", True]]),
    "seq/stop_releases": scenario(2 * BEAT, pattern([60]), division=1.0, gate=1.0,
                                  events=[[10000, "stop"]]),
    "seq/live_key_survives": scenario(4 * BEAT, pattern([64, 64, 67, 64]),
                                      events=[[0, "live_on", 64, 0.9], [70000, "live_off", 64]]),
    "seq/len5_vel": scenario(4 * BEAT, [[k, 1, 60 + k, v] for k, v in enumerate([127, 40, 90, 1, 64])],
                             length=5),
    "seq/transpose_clear": scenario(4 * BEAT, pattern([60, 64, 67, 72]), [[0, 64], [0, 67]],
                                    events=[[30000, "transpose", 0, 5], [60000, "clear"]]),
    "seq/max_chord": scenario(BEAT, pattern([60]), [[0, 61 + i] for i in range(7)], division=1.0),
}


def run(sc):
    inst = SynthInstrument(SR)
    seq, clock = inst.sequencer, inst.clock
    clock.bpm = sc["bpm"]
    g = seq.grid
    g.division, g.length, g.swing = sc["division"], sc["length"], sc["swing"]
    seq.gate, seq.enabled = sc["gate"], sc["enabled"]
    for k, on, n, v in sc["steps"]:
        seq.set_step(k, bool(on), n, v)
    for k, n in sc["extra"]:
        seq.add_note(k, n)
    trace, now = [], [0]
    inst.vm.note_on = lambda n, v: trace.append([now[0], 1, n, round(v * 127)])
    inst.vm.note_off = lambda n: trace.append([now[0], 0, n, 0])
    events = sorted(sc["events"], key=lambda e: e[0])
    total, i, ev = sc["frames"], 0, 0
    while i < total:
        now[0] = i
        while ev < len(events) and events[ev][0] == i:
            apply(inst, events[ev][1:])
            ev += 1
        inst.drain()
        inst.poll()
        nxt = events[ev][0] if ev < len(events) else total
        chunk = min(total - i, inst.frames_to_event(), nxt - i)
        now[0] = i + chunk
        inst.advance(chunk)
        clock.advance(chunk)
        i += chunk
    inst.poll()
    return np.array(trace, dtype=np.float64).reshape(-1, 4)


def apply(inst, cmd):
    seq, op = inst.sequencer, cmd[0]
    if op == "start":
        inst.clock.start()
    elif op == "stop":
        inst.clock.stop()
    elif op == "enable":
        seq.enabled = cmd[1]
    elif op == "live_on":
        inst.note_on(cmd[1], cmd[2])
    elif op == "live_off":
        inst.note_off(cmd[1])
    elif op == "transpose":
        seq.transpose(cmd[1], cmd[2])
    elif op == "clear":
        seq.clear()
    else:
        raise ValueError(op)


def write(out_dir: Path) -> None:
    w = BlobWriter("sequencer")
    for label, sc in PLAY.items():
        w.add(label, run(sc).ravel(), **sc)
    w.write(out_dir)
