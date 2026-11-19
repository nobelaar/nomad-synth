from pathlib import Path

from tools import ref_presets

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    ref_presets.write(tmp_path)
    assert (tmp_path / "presets.json").read_bytes() == (REF / "presets.json").read_bytes()
    for kind in ("synth", "drums"):
        for f in (tmp_path / "preset_files" / kind).iterdir():
            assert f.read_bytes() == (REF / "preset_files" / kind / f.name).read_bytes(), f.name


def test_bad_files_are_not_listed():
    names = [n for n, origin in ref_presets.PresetStore(REF / "preset_files").list("synth") if origin == "user"]
    assert names == ["Acordes de prueba"]
