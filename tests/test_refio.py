import json

import numpy as np

from tools.refio import BlobWriter, regenerate_matches


def _write(out_dir):
    w = BlobWriter("t")
    w.add("a", np.arange(5.0), k=1)
    w.add("b", np.array([0.5]))
    w.write(out_dir)


def test_blob_roundtrip(tmp_path):
    _write(tmp_path)
    data = np.fromfile(tmp_path / "t.f64", dtype="<f8")
    assert data.tolist() == [0.0, 1.0, 2.0, 3.0, 4.0, 0.5]
    entries = json.loads((tmp_path / "t.json").read_text())["entries"]
    assert entries["a"] == {"offset": 0, "len": 5, "meta": {"k": 1}}
    assert entries["b"]["offset"] == 5


def test_non_finite_meta_is_encoded(tmp_path):
    w = BlobWriter("t")
    w.add("a", np.zeros(1), x=float("nan"), y=[float("inf"), -float("inf")])
    w.write(tmp_path)
    meta = json.loads((tmp_path / "t.json").read_text())["entries"]["a"]["meta"]
    assert meta == {"x": "nan", "y": ["inf", "-inf"]}


def test_regenerate_matches_detects_diff(tmp_path):
    ref = tmp_path / "ref"
    _write(ref)
    assert regenerate_matches(_write, ref, tmp_path / "a") == []
    raw = bytearray((ref / "t.f64").read_bytes())
    raw[0] ^= 1
    (ref / "t.f64").write_bytes(bytes(raw))
    assert regenerate_matches(_write, ref, tmp_path / "b") == ["t.f64"]
