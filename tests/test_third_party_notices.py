import shutil
from pathlib import Path

import pytest

from tools import third_party_notices as tpn

ROOT = Path(__file__).resolve().parent.parent


def entry(license, texts=("text",)):
    return {"name": "x", "version": "1.0.0", "license": license, "texts": [["LICENSE", t] for t in texts]}


def test_check_accepts_or_expressions():
    for lic in ["MIT OR Apache-2.0", "MIT/Apache-2.0", "(MIT OR Apache-2.0) AND Unicode-3.0",
                "Apache-2.0 WITH LLVM-exception OR MIT", "Apache-2.0 OR GPL-2.0-only"]:
        assert tpn.check([entry(lic)]) == [], lic


def test_check_rejects_gpl_incompatible():
    for lic in ["SSPL-1.0", "BUSL-1.1", "", None, "MIT AND SSPL-1.0", "(MIT"]:
        assert tpn.check([entry(lic)]), lic


def test_check_requires_license_text():
    assert tpn.check([entry("MIT", texts=())])


def test_render_includes_numpy_entry():
    text = tpn.render([entry("MIT")])
    assert "numpy" in text and "BSD-3-Clause" in text and "NumPy Developers" in text
    assert "## x 1.0.0" in text


@pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo not on PATH")
def test_committed_notices_are_up_to_date():
    entries = tpn.collect(tpn.metadata())
    assert tpn.render(entries) == (ROOT / "THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8")


def test_standard_text_only_for_known_licenses():
    assert [n for n, _ in tpn.standard_texts("MIT OR Apache-2.0", ["Ana"])] == [
        f"MIT ({tpn.STANDARD_MARK})"]
    assert "Authors: Ana" in tpn.standard_texts("Apache-2.0", ["Ana"])[0][1]
    assert tpn.standard_texts("BSD-3-Clause", ["Ana"]) == [], "no standard text: still an error"
    assert tpn.standard_texts("(MIT OR Apache-2.0) AND Unicode-3.0", []) == []


def test_render_writes_each_text_once():
    a, b = entry("MIT", texts=("same",)), entry("MIT", texts=("same",))
    b["name"] = "y"
    out = tpn.render([a, b])
    assert out.count("```text\nsame\n```") == 1
    assert out.count("(#text-1)") == 2


def test_font_exception_is_only_for_egui_fonts():
    fonts = "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0"
    ok = entry(fonts)
    ok["name"] = "epaint_default_fonts"
    assert tpn.check([ok]) == []
    assert tpn.check([entry(fonts)]), "same licenses on another package: rejected"
    sneaky = entry("SSPL-1.0")
    sneaky["name"] = "epaint_default_fonts"
    assert tpn.check([sneaky]), "the exception adds only the font licenses"
    assert tpn.AGGREGATE_NOTE in tpn.render([ok])
