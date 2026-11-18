import json
from pathlib import Path

import numpy as np

from tools import ref_drums
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_drums.write, REF, tmp_path) == []
    meta = json.loads((REF / "drums.json").read_text())["entries"]["drums/default_2bars"]["meta"]
    assert meta["default_pattern"] == ref_drums.default_pattern()


def test_stop_all_goes_silent():
    y = ref_drums.run(ref_drums.MORE["drums/stop_all"])
    assert np.all(y[12000 + 512:18000] == 0.0)
