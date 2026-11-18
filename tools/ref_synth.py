"""SynthInstrument (DSP core) reference scenarios, without engine or sequencer."""

from __future__ import annotations

import json
from itertools import cycle
from pathlib import Path

import numpy as np

from nomad.engine import instrument
from nomad.engine.instrument import SynthInstrument
from nomad.engine.params import REGISTRY
from tools.refio import BlobWriter

SR = 48000
NAN = float("nan")
PATTERN = [256, 256, 100]


def runs(total):
    """["run", n] commands covering ``total`` frames with the block pattern."""
    out, left, blocks = [], total, cycle(PATTERN)
    while left > 0:
        n = min(next(blocks), left)
        out.append(["run", n])
        left -= n
    return out


def params(**kv):
    return [["param", k, v] for k, v in kv.items()]


def sweep():
    cmds = params(filt_reso=0.5) + [["on", 57, 0.8]]
    blocks = runs(5000)
    for k, run in enumerate(blocks):
        cmds.append(["param", "filt_cutoff", 200.0 * 40.0 ** (k / (len(blocks) - 1))])
        cmds.append(run)
    return cmds


PAD = {
    "pad": params(polyphony=6, amp_attack=400.0, amp_release=800.0, lfo1_wave=1, lfo1_rate=0.25,
                  lfo1_depth=1.0, mod0_src=1, mod0_dst=5, mod0_amt=0.6)
    + [["on", n, 0.7] for n in (57, 60, 64, 67)] + runs(14000)
    + [["off", n] for n in (57, 60, 64, 67)] + runs(6000),
}
MORE = {
    "param_sweep": sweep(),
    "controls": params(bend_range=12, mod0_src=3, mod0_dst=1, mod0_amt=0.5) + [["on", 60, 0.8]]
    + runs(1000) + [["ctl", "bend", 1.0]] + runs(1500) + [["ctl", "modwheel", 1.0]] + runs(1500),
    "glide_mono": params(polyphony=1, glide=80.0) + [["on", 60, 0.8]] + runs(1500)
    + [["on", 64, 0.8]] + runs(1500) + [["on", 67, 0.8]] + runs(1500) + [["off", 67]] + runs(500),
    "lfo_sh": params(lfo2_wave=4, lfo2_rate=8.0, lfo2_depth=1.0, mod0_src=2, mod0_dst=6, mod0_amt=1.0,
                     osc1_wave=3) + [["on", 48, 0.8]] + runs(4000),
    # k > 3.2 inside full synth blocks (16-sample grains, state clamp).
    "hi_reso": params(filt_reso=0.9, filt_cutoff=600.0, filt_env_amt=0.4) + [["on", 60, 0.8]] + runs(2000),
    "clamps": params(filt_cutoff=1e9, filt_reso=NAN, osc1_coarse=-100.0, amp_sustain=5.0, polyphony=0.4,
                     mod1_src=9.7, mod1_dst=2.5, mod1_amt=-3.0)
    + [["on", 60, 0.8], ["on", 64, 0.8], ["panic"]] + runs(600) + [["on", 67, 0.8]] + runs(1400),
}


def run(commands):
    s = SynthInstrument(SR)
    out = []
    for cmd in commands:
        op = cmd[0]
        if op == "param":
            s.set_param(cmd[1], cmd[2])
        elif op == "ctl":
            s.set_control(cmd[1], cmd[2])
        elif op == "on":
            s.note_on(cmd[1], cmd[2])
        elif op == "off":
            s.note_off(cmd[1])
        elif op == "panic":
            s.panic()
        else:
            s.drain()
            b = np.zeros(cmd[1])
            s.render_block(b, cmd[1])
            out.append(b)
    return np.concatenate(out)


def _blob(name, scenarios, out_dir):
    w = BlobWriter(name)
    for label, cmds in scenarios.items():
        w.add(label, run(cmds), commands=cmds)
    w.write(out_dir)


def write(out_dir: Path) -> None:
    out_dir = Path(out_dir)
    _blob("synth", PAD, out_dir)
    _blob("synth_more", MORE, out_dir)
    smoothed = list(instrument._SMOOTHED)
    immediate = [n for n in REGISTRY if n not in smoothed and n != "master_volume"]
    text = json.dumps({"smoothed": smoothed, "immediate": immediate}, indent=1)
    (out_dir / "synth_sets.json").write_text(text + "\n")
