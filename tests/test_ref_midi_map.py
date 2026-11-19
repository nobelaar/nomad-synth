from pathlib import Path

from tools import ref_midi_map

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    ref_midi_map.write(tmp_path)
    assert (tmp_path / "midi_map.json").read_bytes() == (REF / "midi_map.json").read_bytes()
    for f in (tmp_path / "midi_map_files").iterdir():
        assert f.read_bytes() == (REF / "midi_map_files" / f.name).read_bytes(), f.name


def test_relearn_keeps_key_position():
    steps = ref_midi_map.run(ref_midi_map.SCENARIOS["relearn_drops_old"], None)
    # Learning cutoff on reso's key (23) replaces it in place: reso has no CC left.
    assert steps[-2]["bindings"] == [["TEST:TEST MIDI 1 24:0", 0, 23, "filt_cutoff"]]
