"""Binary reference blobs for the Rust tests.

A blob is <name>.f64 (all samples concatenated, little-endian float64) plus
<name>.json with {"entries": {label: {"offset", "len", "meta"}}}; offset and
len count samples, entries keep insertion order. Non-finite floats in meta
are written as the strings "nan", "inf" and "-inf" (strict JSON for serde).

Usage: python tools/refio.py rust/reference   (writes the refio_selftest blob)
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np

MAX_BYTES = 200_000  # plan rule: every .f64 stays small


def _encode(x):
    if isinstance(x, float) and not math.isfinite(x):
        return "nan" if x != x else ("inf" if x > 0 else "-inf")
    if isinstance(x, (list, tuple)):
        return [_encode(v) for v in x]
    if isinstance(x, dict):
        return {k: _encode(v) for k, v in x.items()}
    if isinstance(x, np.generic):
        return _encode(x.item())
    return x


class BlobWriter:
    def __init__(self, name: str) -> None:
        self.name = name
        self._chunks: list[np.ndarray] = []
        self._entries: dict[str, dict] = {}
        self._offset = 0

    def add(self, label: str, data, **meta) -> None:
        if label in self._entries:
            raise ValueError(f"duplicate label {label!r}")
        arr = np.ascontiguousarray(data, dtype="<f8").ravel()
        self._entries[label] = {"offset": self._offset, "len": int(arr.size), "meta": _encode(meta)}
        self._chunks.append(arr)
        self._offset += arr.size

    def write(self, out_dir: Path) -> None:
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        raw = (np.concatenate(self._chunks) if self._chunks else np.zeros(0, "<f8")).tobytes()
        if len(raw) >= MAX_BYTES:
            raise ValueError(f"{self.name}.f64 is {len(raw)} bytes, limit {MAX_BYTES}")
        (out_dir / f"{self.name}.f64").write_bytes(raw)
        text = json.dumps({"entries": self._entries}, indent=1, allow_nan=False)
        (out_dir / f"{self.name}.json").write_text(text + "\n")


def regenerate_matches(write_fn, ref_dir: Path, tmp: Path) -> list[str]:
    """Run write_fn(tmp) and list the generated files that differ from ref_dir."""
    tmp = Path(tmp)
    write_fn(tmp)
    stale = []
    for f in sorted(tmp.iterdir()):
        ref = Path(ref_dir) / f.name
        if not ref.exists() or ref.read_bytes() != f.read_bytes():
            stale.append(f.name)
    return stale


def write_selftest(out_dir: Path) -> None:
    w = BlobWriter("refio_selftest")
    w.add("ramp", np.arange(8.0), tag="x")
    w.add("half", np.array([0.5]))
    w.write(out_dir)


if __name__ == "__main__":
    write_selftest(Path(sys.argv[1] if len(sys.argv) > 1 else "rust/reference"))
