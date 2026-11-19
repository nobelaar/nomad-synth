from pathlib import Path

import numpy as np

from tools import ref_clock
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_clock.write, REF, tmp_path) == []


def steps(label):
    return ref_clock.run_track(ref_clock.TRACK[label]).reshape(-1, 2)[:, 0]


def test_scenarios_fire_steps():
    for label in ref_clock.TRACK:
        s = steps(label)
        if label == "track/stopped":
            assert (s[:5] == -1).all()
        else:
            assert (s >= 0).sum() >= 3, label
    # The handover lands on a boundary the old clock already fired: the join waits.
    h = steps("track/handover_on_boundary")
    assert h[3] == -1
