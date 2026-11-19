"""Third-party notices for the Rust app, from ``cargo metadata``.

Usage: python tools/third_party_notices.py --check | --write PATH

Lists every third-party package the binaries can reach (workspace crates and
dev-only dependencies excluded) with its SPDX expression and license texts, and
checks that each expression can be satisfied with GPL-3.0-compatible licenses.
Standard library only.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "rust" / "Cargo.toml"
# Linux first (M4): only what the Linux binary links.
TARGET = "x86_64-unknown-linux-gnu"

ALLOWED = {
    "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC",
    "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "BSL-1.0", "CC0-1.0", "Unlicense", "MPL-2.0", "0BSD",
}
TEXT_PREFIXES = ("license", "copying", "notice")
# Fonts embedded as data, not code: egui's default fonts (Ubuntu-Light, Hack, Noto
# Emoji, icon font) are separate works aggregated in the binary under their own
# licenses (author's decision, M4). Only this package, only these extra licenses.
AGGREGATE_DATA = {"epaint_default_fonts": {"OFL-1.1", "Ubuntu-font-1.0"}}
AGGREGATE_DIR = "fonts"
AGGREGATE_NOTE = ("Fonts distributed as aggregated data under their own licenses "
                  "(not covered by the GPL of NOMAD).")
# Standard texts for crates published without a license file (only these licenses).
STANDARD_DIR = ROOT / "packaging" / "licenses"
STANDARD = ("MIT", "Apache-2.0")
STANDARD_MARK = "texto estándar; el crate no trae archivo"

NUMPY = """Copyright (c) 2005-2025, NumPy Developers.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

    * Redistributions of source code must retain the above copyright
       notice, this list of conditions and the following disclaimer.

    * Redistributions in binary form must reproduce the above
       copyright notice, this list of conditions and the following
       disclaimer in the documentation and/or other materials provided
       with the distribution.

    * Neither the name of the NumPy Developers nor the names of any
       contributors may be used to endorse or promote products derived
       from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
"""


def metadata() -> dict:
    out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET,
                          "--manifest-path", str(MANIFEST)],
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def collect(meta: dict) -> list[dict]:
    """Third-party packages reachable from the workspace through non-dev dependencies."""
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    seen, todo = set(), list(members)
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for d in nodes[pid]["deps"]:
            if any(k["kind"] != "dev" for k in d["dep_kinds"]):
                todo.append(d["pkg"])
    out = []
    for p in meta["packages"]:
        if p["id"] not in seen or p["id"] in members:
            continue
        folder = Path(p["manifest_path"]).parent
        texts = sorted(
            [f.name, f.read_text(encoding="utf-8", errors="replace")]
            for f in folder.iterdir()
            if f.is_file() and f.name.lower().startswith(TEXT_PREFIXES)
        )
        if p["name"] in AGGREGATE_DATA and (folder / AGGREGATE_DIR).is_dir():
            texts += sorted(
                [f"{AGGREGATE_DIR}/{f.name}", f.read_text(encoding="utf-8", errors="replace")]
                for f in (folder / AGGREGATE_DIR).iterdir()
                if f.is_file() and f.suffix == ".txt"
            )
        if not texts:
            texts = standard_texts(p.get("license") or "", p.get("authors") or [])
        out.append({"name": p["name"], "version": p["version"], "license": p["license"], "texts": texts})
    return sorted(out, key=lambda e: (e["name"], e["version"]))


def _tokens(expr: str) -> list[str]:
    raw = expr.replace("(", " ( ").replace(")", " ) ").replace("/", " OR ").split()
    out: list[str] = []
    i = 0
    while i < len(raw):
        if i + 2 < len(raw) and raw[i + 1] == "WITH":
            out.append(f"{raw[i]} WITH {raw[i + 2]}")
            i += 3
        else:
            out.append(raw[i])
            i += 1
    return out


def choose(expr: str, allowed) -> list[str] | None:
    """Licenses of one way to satisfy the SPDX expression with ``allowed``, or None.

    ``A OR B`` takes the first branch that works, ``A AND B`` needs both; AND binds
    tighter. ValueError for a malformed expression.
    """
    toks = _tokens(expr)
    pos = 0

    def atom():
        nonlocal pos
        if pos >= len(toks):
            raise ValueError("unexpected end")
        t = toks[pos]
        pos += 1
        if t == "(":
            v = disj()
            if pos >= len(toks) or toks[pos] != ")":
                raise ValueError("missing )")
            pos += 1
            return v
        if t in ("AND", "OR", ")"):
            raise ValueError(f"unexpected {t}")
        return [t] if t in allowed else None

    def conj():
        nonlocal pos
        v = atom()
        while pos < len(toks) and toks[pos] == "AND":
            pos += 1
            w = atom()
            v = v + w if v is not None and w is not None else None
        return v

    def disj():
        nonlocal pos
        v = conj()
        while pos < len(toks) and toks[pos] == "OR":
            pos += 1
            w = conj()
            v = v if v is not None else w
        return v

    v = disj()
    if pos != len(toks):
        raise ValueError("trailing tokens")
    return v


def satisfiable(expr: str) -> bool:
    return choose(expr, ALLOWED) is not None


def standard_texts(expr: str, authors: list[str]) -> list[list[str]]:
    """The standard texts that satisfy ``expr`` (empty if it needs another license)."""
    try:
        picked = choose(expr, STANDARD) if expr.strip() else None
    except ValueError:
        picked = None
    if not picked:
        return []
    who = ", ".join(authors) or "(sin autores declarados)"
    return [[f"{lic} ({STANDARD_MARK})",
             f"Authors: {who}\n\n" + (STANDARD_DIR / f"{lic}.txt").read_text(encoding="utf-8")]
            for lic in dict.fromkeys(picked)]


def check(entries: list[dict]) -> list[str]:
    errors = []
    for e in entries:
        who = f"{e['name']} {e['version']}"
        lic = (e.get("license") or "").strip()
        if not lic:
            errors.append(f"{who}: no license declared")
            continue
        try:
            ok = choose(lic, ALLOWED | AGGREGATE_DATA.get(e["name"], set())) is not None
        except ValueError as err:
            errors.append(f"{who}: bad license expression {lic!r} ({err})")
            continue
        if not ok:
            errors.append(f"{who}: {lic} is not satisfiable with GPL-3.0-compatible licenses")
        if not e.get("texts"):
            errors.append(f"{who}: no LICENSE/COPYING/NOTICE file found")
    return errors


def render(entries: list[dict]) -> str:
    """Markdown: numpy, one section per package, then each distinct text once."""
    parts = [
        "# Third-party notices\n",
        "NOMAD is distributed under GPL-3.0-or-later (see `LICENSE`). It includes the following "
        "third-party components. This file is generated by `tools/third_party_notices.py`; do not edit.\n",
        "## numpy (ziggurat tables)\n",
        "The normal-distribution tables in `rust/nomad-core/src/ziggurat.rs` come from numpy "
        "(`ziggurat_constants.h`).\n\nLicense: BSD-3-Clause\n",
        "```text\n" + NUMPY + "```\n",
        "# Packages\n",
    ]
    ids: dict[str, int] = {}
    for e in entries:
        refs = []
        for name, text in e["texts"]:
            n = ids.setdefault(text, len(ids) + 1)
            refs.append(f"[{name}](#text-{n})")
        parts.append(f"## {e['name']} {e['version']}\n")
        note = f"{AGGREGATE_NOTE}\n\n" if e["name"] in AGGREGATE_DATA else ""
        parts.append(f"{note}License: {e['license']}\n\nTexts: " + (", ".join(refs) or "none") + "\n")
    parts.append("# License texts\n")
    for text, n in ids.items():
        parts.append(f'<a id="text-{n}"></a>\n\n## Text {n}\n')
        parts.append("```text\n" + text.rstrip("\n").replace("```", "\'\'\'") + "\n```\n")
    return "\n".join(parts)


def main() -> int:
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--check", action="store_true")
    g.add_argument("--write", metavar="PATH")
    args = ap.parse_args()
    entries = collect(metadata())
    errors = check(entries)
    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    if args.write:
        Path(args.write).write_text(render(entries), encoding="utf-8")
        print(f"wrote {args.write} ({len(entries)} packages)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
