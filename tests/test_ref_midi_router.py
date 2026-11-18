from pathlib import Path

from tools import ref_midi_router
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_midi_router.write, REF, tmp_path) == []


def test_sustain_releases_on_pedal_up():
    t = ref_midi_router.run(ref_midi_router.SCENARIOS["sustain_hold_release"])
    assert t[-3:] == [["note_off", 60], ["note_off", 64], ["note_off", 67]]
