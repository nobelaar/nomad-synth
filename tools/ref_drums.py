"""DrumMachine reference: its standalone render driver in 512-frame blocks.

Tone changes (tune/decay) are applied deterministically here: the value is
stored and the one-shot regenerated at once (a ("bank", track) command follows),
bypassing the GUI-side throttle. Audio is kept 1 in DECIM plus full checksums.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np

from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS, DrumMachine, _clamp, render_one_shot
from tools.refio import BlobWriter

SR = 48000.0
BLOCK, DECIM = 512, 20
BAR = 96000  # frames per 4/4 bar at 120 bpm


def with_bank(cmds):
    out = []
    for c in cmds:
        out.append(c)
        if c[0] == "set" and c[1].rsplit("_", 1)[-1] in ("tune", "decay"):
            out.append(["bank", INSTRUMENTS.index(c[1].split("_")[0])])
    return out


BPM120 = [["set", "bpm", 120.0]]
MAIN = {
    "drums/default_2bars": BPM120 + [["start"], ["render", 2 * BAR]],
    "drums/accents": BPM120 + [["clear"], ["step", "kick", 0, 2], ["step", "kick", 8, 1], ["step", "snare", 4, 2],
                               ["step", "snare", 12, 1], ["step", "ohat", 2, 2], ["step", "ohat", 10, 1],
                               ["step", "tom", 14, 5], ["start"], ["render", BAR]],
}
MORE = {
    "drums/swing": BPM120 + [["set", "swing", 0.4], ["start"], ["render", BAR]],
    "drums/tune_decay": with_bank(BPM120 + [["start"], ["render", BAR // 2], ["set", "kick_tune", 0.8],
                                            ["set", "snare_decay", 0.1], ["render", BAR // 2]]),
    "drums/stop_all": BPM120 + [["start"], ["render", 12000], ["stop_all"], ["render", 6000], ["start"],
                                ["render", 12000]],
    "drums/trigger_manual": [["trigger", "clap", 1.0], ["render", 4096], ["trigger", "kick", 0.5],
                             ["trigger", "ohat", 2.0], ["render", 20480]],
    "drums/levels": BPM120 + [["set", "kick_level", 0.2], ["set", "chat_level", 1.0], ["set", "snare_level", -3.0],
                              ["trigger", "tom", float("nan")], ["start"], ["render", BAR]],
}


def run(commands):
    dm = DrumMachine(SR)
    out = []
    for c in commands:
        op = c[0]
        if op == "set":
            name, v = c[1], c[2]
            if name.rsplit("_", 1)[-1] in ("tune", "decay"):
                dm.values[name] = _clamp(DRUM_REGISTRY[name], v)
            else:
                dm.set_value(name, v)
        elif op == "bank":
            t = INSTRUMENTS[c[1]]
            dm._bank[c[1]] = render_one_shot(t, dm.sr, dm.values[f"{t}_tune"], dm.values[f"{t}_decay"])
        elif op == "start":
            dm.clock.start()
        elif op == "trigger":
            dm.trigger(c[1], c[2])
        elif op == "step":
            dm.set_step(c[1], c[2], c[3])
        elif op == "clear":
            dm.clear_pattern()
        elif op == "stop_all":
            dm.stop_all()
        else:
            left = c[1]
            while left > 0:  # 512-frame blocks, the last one partial
                n = min(BLOCK, left)
                b = np.zeros(n)
                dm.render(b, n)
                out.append(b)
                left -= n
    return np.concatenate(out)


def write(out_dir: Path) -> None:
    for name, scenarios in (("drums", MAIN), ("drums_more", MORE)):
        w = BlobWriter(name)
        for label, cmds in scenarios.items():
            y = run(cmds)
            w.add(label, y[::DECIM], commands=cmds, block=BLOCK, decim=DECIM, len=int(y.size),
                  sum=math.fsum(y), sumsq=math.fsum(y * y), default_pattern=default_pattern())
        w.write(out_dir)


def default_pattern() -> list[list[int]]:
    return DrumMachine(SR).steps.tolist()
