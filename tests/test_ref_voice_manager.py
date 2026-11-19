from pathlib import Path

from tools import ref_voice_manager
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_voice_manager.write, REF, tmp_path) == []


def test_scenarios_mean_what_they_say():
    _, checks = ref_voice_manager.run(*ref_voice_manager.SCENARIOS["steal_releasing"])
    assert 62 not in checks[-1] and 80 in checks[-1] and 60 in checks[-1]
    _, checks = ref_voice_manager.run(*ref_voice_manager.SCENARIOS["poly_lower"])
    assert checks[-1] == [67, 71]
    _, checks = ref_voice_manager.run(*ref_voice_manager.SCENARIOS["glide_mono_legato"])
    assert checks == [[60], [67], []]
