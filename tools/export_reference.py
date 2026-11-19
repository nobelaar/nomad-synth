"""Dump the Python parameter registry and Smoother traces for the Rust tests.

Usage: python tools/export_reference.py rust/reference
Reads nomad/ without modifying it; writes <out_dir>/params.json.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from nomad.engine import drums  # noqa: E402
from nomad.engine.params import REGISTRY, Smoother, clamp, from_norm, to_norm  # noqa: E402
from tools import ref_clock, ref_gui_logic, ref_keyboard, ref_drum_shots, ref_drums, ref_engine, ref_envelope, ref_filter, ref_lfo, ref_midi_map, ref_midi_parse, ref_midi_router, ref_modmatrix, ref_normal, ref_oscillator, ref_presets, ref_seq_view, ref_sequencer, ref_synth, ref_synth_layout, ref_synth_seq, ref_topology, ref_voice, ref_voice_manager  # noqa: E402

FROM_NORM_IN = [-0.5, 0.0, 0.25, 0.5, 1.0, 1.5]
SMOOTHER_CASES = [
    (0.0, 1.0, 48000.0, 15.0, [64, 64, 256, 512, 0]),
    (0.25, 0.75, 44100.0, 0.0, [1, 16, 0]),
]


def _spec(spec) -> dict:
    return {
        "name": spec.name,
        "default": spec.default,
        "lo": spec.lo,
        "hi": spec.hi,
        "curve": spec.curve,
        "unit": spec.unit,
        "smooth_ms": spec.smooth_ms,
        "clamp_in": [-1e9, spec.lo, (spec.lo + spec.hi) / 2, spec.hi, 1e9],
    }


def _synth(spec) -> dict:
    d = _spec(spec)
    d["clamp_out"] = [clamp(spec.name, x) for x in d["clamp_in"]]
    d["to_norm_out"] = [to_norm(spec.name, x) for x in d["clamp_in"]]
    d["from_norm_in"] = FROM_NORM_IN
    d["from_norm_out"] = [from_norm(spec.name, n) for n in FROM_NORM_IN]
    return d


def _drum(spec) -> dict:
    d = _spec(spec)
    d["clamp_out"] = [drums._clamp(spec, x) for x in d["clamp_in"]]
    return d


def _smoother(start, target, sr, ms, steps) -> dict:
    s = Smoother(start, sr, ms)
    s.set(target)
    values = [s.advance(n) for n in steps]
    return {"start": start, "target": target, "sample_rate": sr, "time_ms": ms,
            "steps": steps, "values": values}


def build_params() -> dict:
    return {
        "version": 1,
        "synth": [_synth(s) for s in REGISTRY.values()],
        "drums": [_drum(s) for s in drums.DRUM_REGISTRY.values()],
        "smoother": [_smoother(*c) for c in SMOOTHER_CASES],
    }


def write(out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    # allow_nan=False enforces "no non-finite values in the file".
    text = json.dumps(build_params(), indent=1, allow_nan=False)
    (out_dir / "params.json").write_text(text + "\n")
    ref_oscillator.write(out_dir)
    ref_envelope.write(out_dir)
    ref_filter.write(out_dir)
    ref_lfo.write(out_dir)
    ref_modmatrix.write(out_dir)
    ref_voice.write(out_dir)
    ref_voice_manager.write(out_dir)
    ref_synth.write(out_dir)
    ref_clock.write(out_dir)
    ref_sequencer.write(out_dir)
    ref_synth_seq.write(out_dir)
    ref_normal.write(out_dir)
    ref_drum_shots.write(out_dir)
    ref_drums.write(out_dir)
    ref_engine.write(out_dir)
    ref_topology.write(out_dir)
    ref_midi_parse.write(out_dir)
    ref_midi_router.write(out_dir)
    ref_midi_map.write(out_dir)
    ref_gui_logic.write(out_dir)
    ref_synth_layout.write(out_dir)
    ref_seq_view.write(out_dir)
    ref_keyboard.write(out_dir)
    ref_presets.write(out_dir)


def main() -> None:
    write(Path(sys.argv[1] if len(sys.argv) > 1 else "rust/reference"))


if __name__ == "__main__":
    main()
