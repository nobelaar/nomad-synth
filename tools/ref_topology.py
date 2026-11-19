"""Topology reference: Engine + InstrumentManager operations with follower clocks.

Each ("render", n) renders n frames in Engine.render calls of at most CALL
frames (like tests/test_follow.py::run); it stores 1 in DECIM samples, per-render
checksums and the (pos, bpm, running) of every instance's own clock in mix
order. After EVERY operation the manager's (id, name) list is stored. Clock
states are taken after renders only: Python adopts a published topology at the
next render, Rust applies it at once, and both agree from the render on.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np

from nomad.engine.engine import Engine
from nomad.engine.manager import InstrumentManager
from tools.refio import BlobWriter

SR = 48000
CALL, DECIM = 4096, 20
BEAT128 = 5625 * 4  # frames per beat at 128 bpm (a 16th is 5625)


def follow(bpm):
    return [["hats", 2], ["seq_on", 1], ["bpm", 2, bpm], ["source", 1, 2], ["start", 2]]


BLOBS = {
    "topology": {
        "topo/follow_master": follow(128.0) + [["render", 3 * 16 * 5625]],
    },
    "topology_b": {
        "topo/bpm_change_reaches_follower": follow(120.0) + [["render", 20000], ["bpm", 2, 128.0],
                                                             ["render", 60000]],
        "topo/different_lengths": [["length", 1, 12]] + follow(128.0) + [["render", 30 * 5625]],
    },
    "topology_c": {
        "topo/close_master_handover": follow(128.0) + [["render", 30000], ["remove", 2], ["render", 30000],
                                                       ["drop_closed"], ["render", 20000]],
        "topo/go_internal_keeps_tempo": [["seq_on", 1], ["bpm", 2, 140.0], ["source", 1, 2], ["start", 2],
                                         ["render", 25000], ["source", 1, None], ["render", 25000],
                                         ["bpm", 1, 90.0], ["render", 10000]],
        "topo/rules": [["add", "synth"], ["add", "drums"], ["source", 1, 2], ["source", 1, 1], ["source", 1, 99],
                       ["source", 2, 4], ["source", 1, 2], ["source", 3, 1], ["start", 1], ["bpm", 1, 100.0],
                       ["remove", 4], ["source", 3, 4], ["source", 99, 2], ["render", 1024]],
        "topo/add_names_ids": [["add", "synth"], ["add", "drums"], ["add", "synth"], ["remove", 3],
                               ["drop_closed"], ["add", "synth"], ["render", 512]],
        "topo/closing_fade": [["note_on", 60, 0.9], ["note_on", 64, 0.9], ["render", 4096], ["remove", 1],
                              ["render", 2048], ["drop_closed"], ["render", 2048]],
    },
}


def run(commands):
    e = Engine(SR)
    pending = []
    m = InstrumentManager(e, schedule=lambda _ms, fn: pending.append(fn))

    def find(i):
        return next((x for x in m.instances if x.id == i), None)

    audio, sums, clocks, rets, names = [], [], [], [], []
    for c in commands:
        op = c[0]
        if op == "add":
            m.add(c[1])
        elif op == "source":
            inst, src = find(c[1]), (find(c[2]) if c[2] is not None else None)
            if inst is None or (c[2] is not None and src is None):
                rets.append(False)  # unknown id: the manager would raise; Rust returns false
            else:
                rets.append(m.set_source(inst, src))
        elif op == "remove":
            m.remove(find(c[1]))
        elif op == "drop_closed":
            fns, pending[:] = list(pending), []
            for fn in fns:
                fn()
        elif op == "start":
            rets.append(m.start(find(c[1])))
        elif op == "bpm":
            rets.append(m.set_bpm(find(c[1]), c[2]))
        elif op == "note_on":
            e.note_on(c[1], c[2])
        elif op == "seq_on":
            sq = find(c[1]).sequencer
            sq.enabled = True
            for s in range(16):
                sq.set_step(s, True, note=48 + s)
        elif op == "hats":
            d = find(c[1])
            d.clear_pattern()
            for s in range(16):
                d.set_step("chat", s, 1)
        elif op == "length":
            find(c[1]).sequencer.grid.length = c[2]
        elif op == "render":
            o = np.zeros(c[1])
            for s in range(0, c[1], CALL):
                e.render(o[s:s + CALL])
            audio.append(o)
            sums.append([math.fsum(o), math.fsum(o * o)])
            clocks.append([[i.clock.pos, i.clock.bpm, float(i.clock.running)] for i in e.topology.insts])
        else:
            raise ValueError(op)
        names.append([[i.id, i.name] for i in m.instances])
    return np.concatenate(audio), sums, clocks, rets, names


def write(out_dir: Path) -> None:
    for blob, scenarios in BLOBS.items():
        w = BlobWriter(blob)
        for label, cmds in scenarios.items():
            y, sums, clocks, rets, names = run(cmds)
            w.add(label, y[::DECIM], commands=cmds, decim=DECIM, call=CALL, sums=sums, clocks=clocks,
                  returns=rets, names=names)
        w.write(out_dir)
