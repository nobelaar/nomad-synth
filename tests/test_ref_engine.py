from pathlib import Path

import numpy as np

from nomad.engine.engine import Engine
from tools import ref_engine
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_engine.write, REF, tmp_path) == []


def test_golden_recipe_reproduces_the_golden():
    # Same recipe as tests/test_engine_mixer.py::test_default_level_is_bit_identical_to_before.
    e = Engine(48000)
    e.drums.set_value("bpm", 120.0)
    e.drums.start()
    e.note_on(60, 0.8)
    e.note_on(67, 0.6)
    out = []
    for n in ref_engine.GOLDEN_SIZES:
        o = np.zeros(n)
        e.render(o)
        out.append(o)
        if len(out) == 4:
            e.note_off(60)
            e.note_on(64, 1.0)
    assert np.abs(np.concatenate(out) - ref_engine.golden()).max() < 1e-12
