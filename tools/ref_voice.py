"""Voice reference scenarios: params overrides, mods and a command list."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from nomad.engine.params import REGISTRY
from nomad.engine.voice import Voice
from tools.refio import BlobWriter

SR = 48000.0
NAN = float("nan")
MOD_KEYS = ["pitch", "pitch1", "pitch2", "amp", "cutoff", "pw1", "pw2", "bend"]
ON60 = [["on", 60, 0.8], ["run", 1200], ["off"], ["run", 600]]

# label: (param overrides, mods, commands, block). Block 16 as in the plan; filter_env
# uses 256 so the 64-sample filter grain is exercised too.
VOICE = {
    "basic/saw": ({}, {}, ON60, 16),
    "basic/sine_tri_sq": ({"osc1_wave": 0.0, "osc2_wave": 3.0, "osc2_level": 0.7, "osc2_coarse": -12.0},
                          {}, ON60, 16),
    "sync": ({"osc2_sync": 1.0, "osc2_coarse": 7.0, "osc1_level": 0.2}, {}, ON60, 16),
    "free_mode": ({"osc1_mode": 1.0, "osc1_hz": 330.0}, {}, ON60, 16),
    "filter_env": ({"filt_env_amt": 0.8, "filt_reso": 0.6, "filt_cutoff": 400.0}, {}, ON60, 256),
    "mods": ({"osc1_wave": 3.0}, {"pitch": 0.5, "pitch1": -1.2, "cutoff": 0.3, "pw1": 0.2,
                                  "amp": -0.3, "bend": 2.0}, ON60, 16),
    "mods_extreme": ({}, {"pitch": 1000.0, "cutoff": NAN, "amp": -5.0},
                     [["on", 60, 0.8], ["run", 800]], 16),
}
GLIDE = {
    "steal": ({}, {}, [["on", 60, 0.8], ["run", 2000], ["steal", 72, 0.6], ["run", 1500]], 16),
    "retrigger": ({}, {}, [["on", 60, 0.8], ["run", 1000], ["steal", 60, 0.9], ["run", 1000]], 16),
    "glide/fresh": ({"glide": 200.0}, {}, [["on", 72, 0.8, 60.0], ["run", 3000]], 16),
    "glide/steal": ({"glide": 300.0}, {}, [["on", 60, 0.8], ["run", 1000], ["steal", 72, 0.8],
                                          ["run", 3000]], 16),
    "glide/legato": ({"glide": 100.0}, {}, [["on", 60, 0.8], ["run", 1500], ["target", 67],
                                           ["run", 3000]], 16),
    "vel/nan": ({}, {}, [["on", 60, NAN], ["run", 400]], 16),
    "vel/neg": ({}, {}, [["on", 60, -1.0], ["run", 400]], 16),
    "vel/big": ({}, {}, [["on", 60, 7.0], ["run", 400]], 16),
}


def params(overrides):
    p = {name: spec.default for name, spec in REGISTRY.items()}
    p.update(overrides)
    return p


def run(overrides, mods, commands, block):
    v = Voice(SR, 0)
    p = params(overrides)
    m = {k: 0.0 for k in MOD_KEYS}
    m.update(mods)
    out = []
    for cmd in commands:
        op = cmd[0]
        if op == "on":
            v.note_on(cmd[1], cmd[2], cmd[3] if len(cmd) > 3 else None)
        elif op == "off":
            v.note_off()
        elif op == "steal":
            v.begin_steal(cmd[1], cmd[2])
        elif op == "target":
            v.set_target(cmd[1])
        else:
            left = cmd[1]
            while left > 0:
                buf = np.zeros(min(block, left))
                v.render(buf, p, m)
                out.append(buf)
                left -= buf.size
    return np.concatenate(out), v.is_active()


def _blob(name, scenarios, out_dir):
    w = BlobWriter(name)
    for label, (ov, mods, cmds, block) in scenarios.items():
        data, active = run(ov, mods, cmds, block)
        w.add(label, data, params=ov, mods=mods, commands=cmds, block=block, active_end=active)
    w.write(out_dir)


def write(out_dir: Path) -> None:
    _blob("voice", VOICE, out_dir)
    _blob("voice_glide", GLIDE, out_dir)
