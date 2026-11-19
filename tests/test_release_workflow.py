import re
from pathlib import Path

WORKFLOW = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release.yml"


def text():
    return WORKFLOW.read_text(encoding="utf-8")


def test_only_version_tags_trigger_it():
    t = text()
    assert re.search(r"tags:\s*\n\s*- ['\"]?v\*['\"]?", t), "v* tags"
    assert "branches" not in t and "pull_request" not in t and "workflow_dispatch" not in t
    assert "push:" in t


def test_runs_on_ubuntu_22_04_with_alsa():
    t = text()
    assert "runs-on: ubuntu-22.04" in t
    assert "libasound2-dev" in t


def test_gate_notices_and_version_check_before_publishing():
    t = text()
    order = [t.index(s) for s in (
        "cargo fmt --all -- --check",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo test --workspace",
        "third_party_notices.py --check",
        "GITHUB_REF_NAME",
        "packaging/linux/build.sh",
        "gh release create",
    )]
    assert order == sorted(order)
    assert "contents: write" in t
    assert "--generate-notes" in t
    assert "rust/target/dist/" in t
