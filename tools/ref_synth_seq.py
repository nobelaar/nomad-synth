"""Synth + sequencer audio through the Engine.render scheduling loop (one instance).

The loop mirrors Engine.render/_span for a single Internal instance at unity
level: drain at the start of each 512-frame callback, then poll / chunk to the
next event / 256-frame render blocks / advance / clock.advance, and a final
poll. No master gain or limiter (that is the mixer, M1-D). Audio is stored
decimated by DECIM to stay under the blob size limit; any divergence would
propagate through the voices' state, so this still checks the whole render.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np

from nomad import presets
from nomad.engine.instance import CONTROL_BLOCK
from nomad.engine.instrument import SynthInstrument
from tools.refio import BlobWriter

SR = 48000
CALLBACK = 512
DECIM = 20


def render_callback(inst, out):
    inst.drain()
    total, i = out.shape[0], 0
    while i < total:
        inst.poll()
        chunk = min(total - i, inst.frames_to_event())
        for s in range(i, i + chunk, CONTROL_BLOCK):
            n = min(CONTROL_BLOCK, i + chunk - s)
            b = np.zeros(n)
            inst.render_block(b, n)
            out[s:s + n] = b
        inst.advance(chunk)
        inst.clock.advance(chunk)
        i += chunk
    inst.poll()


def factory(name):
    return next(p for p in presets.FACTORY["synth"] if p["name"] == name)


def chords_rec():
    ev, cb = [[0, "arm", True]], 1
    for chord in ([48, 52, 55], [45, 48, 52], [41, 45, 48], [43, 47, 50]):
        ev += [[cb, "on", n, 0.8] for n in chord]
        ev += [[cb + 2, "off", n] for n in chord]
        cb += 4
    return ev + [[cb, "arm", False], [cb, "length", 4], [cb, "division", 1.0], [cb, "enable", True],
                 [cb, "start"]]


# label: (preset or None, bpm, seconds, blob, events [callback, cmd...])
SCENARIOS = {
    "audio/pad_progression": ("Pad de acordes", 100.0, 6.0, "synth_seq", [[0, "enable", True], [0, "start"]]),
    "audio/bass_glide": ("Bajo", 100.0, 4.0, "synth_seq_more", [[0, "enable", True], [0, "start"]]),
    "audio/rec_then_play": (None, 120.0, 3.0, "synth_seq_more", chords_rec()),
    "audio/live_key_over_seq": ("Pluck", 110.0, 2.0, "synth_seq_more",
                                [[0, "enable", True], [0, "start"], [10, "on", 57, 0.7], [120, "off", 57]]),
}


def run(preset, bpm, seconds, events):
    inst = SynthInstrument(SR)
    applied = []
    if preset is not None:
        orig = inst.set_param
        inst.set_param = lambda n, v: (applied.append([n, v]), orig(n, v))
        presets.apply(inst, factory(preset))
        inst.set_param = orig
    sq = inst.sequencer
    seq = {"on": sq.on.tolist(), "note": sq.note.tolist(), "vel": sq.vel.tolist(), "extra": sq.extra.tolist(),
           "gate": sq.gate, "division": sq.grid.division, "length": sq.grid.length, "swing": sq.grid.swing}
    inst.clock.bpm = bpm
    n_cb = int(seconds * SR) // CALLBACK
    out = np.zeros(n_cb * CALLBACK)
    for cb in range(n_cb):
        for e in events:
            if e[0] == cb:
                apply_event(inst, e[1:])
        render_callback(inst, out[cb * CALLBACK:(cb + 1) * CALLBACK])
    return out, applied, seq, n_cb


def apply_event(inst, cmd):
    op, sq = cmd[0], inst.sequencer
    if op == "on":
        inst.note_on(cmd[1], cmd[2])
    elif op == "off":
        inst.note_off(cmd[1])
    elif op == "arm":
        inst.set_rec(cmd[1])
    elif op == "enable":
        sq.enabled = cmd[1]
    elif op == "length":
        sq.grid.length = cmd[1]
    elif op == "division":
        sq.grid.division = cmd[1]
    elif op == "start":
        inst.clock.start()
    else:
        raise ValueError(op)


def write(out_dir: Path) -> None:
    blobs = {}
    for label, (preset, bpm, seconds, blob, events) in SCENARIOS.items():
        out, applied, seq, n_cb = run(preset, bpm, seconds, events)
        w = blobs.setdefault(blob, BlobWriter(blob))
        # Checksums of every sample, so the decimated blob still verifies the full render.
        w.add(label, out[::DECIM], sum=math.fsum(out), sumsq=math.fsum(out * out), params=applied, seq=seq, bpm=bpm, callbacks=n_cb, callback=CALLBACK,
              decim=DECIM, events=events)
    for w in blobs.values():
        w.write(out_dir)
