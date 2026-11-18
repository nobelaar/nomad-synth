"""VoiceManager reference scenarios: events between 16-sample blocks."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.voice_manager import VoiceManager
from tools.ref_voice import MOD_KEYS, params
from tools.refio import BlobWriter

SR = 48000.0
BLOCK = 16


def chord(notes, vel=0.8):
    return [["on", n, vel] for n in notes]


EIGHT = chord(range(60, 68))
SCENARIOS = {
    "chord8": (EIGHT + [["run", 1500]], None),
    "steal_oldest": (EIGHT + [["run", 800], ["on", 80, 0.8], ["run", 1200]], None),
    "steal_releasing": (EIGHT + [["run", 800], ["off", 62], ["run", 400], ["on", 80, 0.8],
                                 ["run", 1000]], None),
    "poly1": ([["poly", 1]] + chord([60, 64, 67]) + [["run", 1500]], None),
    "poly_lower": (chord([60, 64, 67, 71]) + [["run", 600], ["poly", 2], ["run", 1400]], None),
    "glide_poly": ([["glide", 150.0], ["on", 55, 0.8], ["run", 400], ["off", 55], ["run", 400]]
                   + chord([60, 64, 67]) + [["run", 1500]], None),
    "glide_mono_legato": ([["poly", 1], ["glide", 100.0], ["on", 60, 0.8], ["run", 1000],
                           ["on", 67, 0.8], ["run", 1500], ["off", 67], ["run", 800]], None),
    "panic": (chord([60, 64, 67, 71]) + [["run", 600], ["panic"], ["run", 800]], None),
    "invalid_notes": ([["on", 128, 0.8], ["on", -1, 0.8], ["on", 60, 0.8], ["run", 800],
                       ["off", 128], ["run", 16]], None),
    "resolve": (chord([60, 64, 67]) + [["run", 1500]], -0.5),
}


def run(commands, resolve_amp):
    vm = VoiceManager(SR)
    p = params({})
    m = {k: 0.0 for k in MOD_KEYS}
    resolve = None
    if resolve_amp is not None:
        fixed = dict(m, amp=resolve_amp)

        def resolve(_voice, _mods):
            return fixed

    out, checks = [], []
    for cmd in commands:
        op = cmd[0]
        if op == "on":
            vm.note_on(cmd[1], cmd[2])
        elif op == "off":
            vm.note_off(cmd[1])
        elif op == "poly":
            vm.set_polyphony(cmd[1])
        elif op == "glide":
            vm.set_glide(cmd[1])
        elif op == "panic":
            vm.panic()
        else:
            buf = np.zeros(cmd[1])
            for i in range(0, cmd[1], BLOCK):
                vm.render(buf[i:i + BLOCK], p, m, resolve)
            out.append(buf)
            checks.append(sorted(vm.active_notes()))
    return np.concatenate(out), checks


def write(out_dir: Path) -> None:
    w = BlobWriter("voice_manager")
    for label, (cmds, amp) in SCENARIOS.items():
        data, checks = run(cmds, amp)
        w.add(label, data, commands=cmds, resolve_amp=amp, block=BLOCK, checks=checks)
    w.write(out_dir)
