from pathlib import Path

from tools import ref_sequencer
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_sequencer.write, REF, tmp_path) == []


def offs(label, note):
    t = ref_sequencer.run(ref_sequencer.PLAY[label])
    return [int(f) for f, on, n, _ in t if not on and n == note]


def test_scenarios_mean_what_they_say():
    # The live key 64 is never cut by the sequencer; only the player's release.
    o = offs("seq/live_key_survives", 64)
    assert [f for f in o if f < 70000] == [] and o[0] == 70000
    # Stopping the clock releases the hanging note at the stop frame.
    assert offs("seq/stop_releases", 60) == [10000]
    t = ref_sequencer.run(ref_sequencer.PLAY["seq/max_chord"])
    assert sum(1 for f, on, *_ in t if on and f == 0) == 6
