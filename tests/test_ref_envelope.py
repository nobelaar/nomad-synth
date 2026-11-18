from pathlib import Path

from tools import ref_envelope
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_envelope.write, REF, tmp_path) == []


def test_basic_ends_idle():
    _, idle = ref_envelope.run(ref_envelope.BASIC["adsr/basic"])
    assert idle
