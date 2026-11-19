from pathlib import Path

from tools import ref_help

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    ref_help.write(tmp_path)
    assert (tmp_path / "help.json").read_bytes() == (REF / "help.json").read_bytes()
