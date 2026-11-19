import hashlib
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUILD = ROOT / "packaging" / "linux" / "build.sh"
APPRUN = ROOT / "packaging" / "linux" / "AppRun"
DIST = ROOT / "rust" / "target" / "dist"


def test_build_script_shape():
    text = BUILD.read_text(encoding="utf-8")
    assert text.startswith("#!/usr/bin/env bash\n")
    assert "set -euo pipefail" in text
    assert "sudo" not in text
    m = re.search(r'^APPIMAGETOOL_SHA256="([0-9a-f]*)"$', text, re.M)
    assert m and len(m.group(1)) == 64, "pinned appimagetool hash"
    assert re.search(r'^APPIMAGETOOL_URL="https://[^"]+/1\.9\.0/[^"]+"$', text, re.M), "pinned version"
    assert "sha256sum" in text and "--skip-build" in text


def test_apprun_runs_the_binary():
    text = APPRUN.read_text(encoding="utf-8")
    assert 'exec "$HERE/usr/bin/nomad" "$@"' in text
    assert APPRUN.stat().st_mode & 0o111, "executable"


def test_sha256sums_match_artifacts():
    sums = DIST / "SHA256SUMS"
    if not sums.exists():
        return  # nothing built here
    for line in sums.read_text().splitlines():
        digest, name = line.split(maxsplit=1)
        f = DIST / name.lstrip("*")
        assert hashlib.sha256(f.read_bytes()).hexdigest() == digest, name
