"""The Rust reference file must match what the Python registry produces."""

import json
from pathlib import Path

from tools import export_reference

REF = Path(__file__).resolve().parent.parent / "rust" / "reference" / "params.json"


def test_reference_file_is_up_to_date():
    # Fails when params.py/drums.py change without regenerating the reference.
    assert json.loads(REF.read_text()) == export_reference.build_params()


def test_reference_shape():
    data = export_reference.build_params()
    synth = data["synth"]
    assert len(synth) == 63
    assert len(data["drums"]) == 20
    assert [p["name"] for p in synth][:3] == ["polyphony", "glide", "master_volume"]
    values = data["smoother"][0]["values"]
    assert values[-1] == values[-2]
