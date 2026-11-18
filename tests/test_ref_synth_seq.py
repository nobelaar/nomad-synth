from pathlib import Path

import numpy as np

from tools import ref_synth_seq
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_synth_seq.write, REF, tmp_path) == []


def test_rec_then_play_records_four_chords():
    sc = ref_synth_seq.SCENARIOS["audio/rec_then_play"]
    out, _, _, _ = ref_synth_seq.run(sc[0], sc[1], sc[2], sc[4])
    assert np.abs(out[-48000:]).max() > 0.05  # the recorded pattern plays
