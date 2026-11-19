"""Envelope reference scenarios: command lists replayed by the Rust tests."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.envelope import Envelope
from tools.refio import BlobWriter

SR = 48000.0
NAN = float("nan")

# The final release of adsr/basic is 10000 samples instead of 20000 to stay under
# the 200 KB blob limit; a 0.2 s release from 0.7 reaches the floor at ~9200.
BASIC = {
    "adsr/basic": [["params", 0.01, 0.1, 0.7, 0.2], ["on"], ["run", 2000], ["run", 6000],
                   ["off"], ["run", 10000]],
}
MORE = {
    "adsr/retrigger": [["on"], ["run", 300], ["off"], ["run", 200], ["on"], ["run", 4000]],
    "adsr/release_from_attack": [["on"], ["run", 100], ["off"], ["run", 3000]],
    "adsr/params_every_block": [["on"]]
    + [c for _ in range(60) for c in (["params", 0.02, 0.1, 0.5, 0.2], ["run", 64])],
    "adsr/times_clamped": [["params", NAN, 0.0, -1.0, 0.2], ["on"], ["run", 200]],
    "adsr/sustain_clamped": [["params", 0.01, 0.1, NAN, 0.2], ["on"], ["run", 500],
                             ["params", 0.01, 0.1, 2.0, 0.2], ["run", 500],
                             ["params", 0.01, 0.1, -1.0, 0.2], ["run", 500]],
}


def run(commands):
    env = Envelope(SR)
    out = []
    for cmd in commands:
        if cmd[0] == "params":
            env.set_params(*cmd[1:])
        elif cmd[0] == "on":
            env.note_on()
        elif cmd[0] == "off":
            env.note_off()
        else:
            buf = np.zeros(cmd[1])
            env.process(buf)
            out.append(buf)
    return np.concatenate(out), env.is_idle()


def _blob(name, scenarios, out_dir):
    w = BlobWriter(name)
    for label, cmds in scenarios.items():
        data, idle = run(cmds)
        w.add(label, data, commands=cmds, idle_end=idle)
    w.write(out_dir)


def write(out_dir: Path) -> None:
    _blob("envelope", BASIC, out_dir)
    _blob("envelope_more", MORE, out_dir)
