from pathlib import Path

from tools import ref_synth_layout
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_synth_layout.write, REF, tmp_path) == []
