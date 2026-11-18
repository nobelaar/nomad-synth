from pathlib import Path

from tools import ref_midi_parse
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_midi_parse.write, REF, tmp_path) == []


def test_extremes():
    ev = {tuple(c): ref_midi_parse.event(c) for c in ref_midi_parse.cases()}
    assert ev[(0xE0, 0, 0)]["bend"] == -1.0 and ev[(0xE0, 0x7F, 0x7F)]["bend"] == 1.0
    assert ev[(0x90, 60, 0)]["kind"] == "note_off"
    assert ev[(0xF8,)] is None and ev[(0x90, 60)] is None
