import shutil
import subprocess
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESKTOP = ROOT / "packaging" / "linux" / "nomad.desktop"
ICON = ROOT / "packaging" / "linux" / "nomad.svg"


def entries():
    lines = DESKTOP.read_text(encoding="utf-8").splitlines()
    assert lines[0] == "[Desktop Entry]"
    return dict(line.split("=", 1) for line in lines[1:] if "=" in line)


def test_desktop_entry_keys():
    e = entries()
    assert e["Type"] == "Application"
    assert e["Name"] == "NOMAD Synth"
    assert e["Comment"] == "Sintetizador de inspiración analógica"
    assert e["Exec"] == "nomad" and e["Icon"] == "nomad"
    assert e["Categories"] == "AudioVideo;Audio;Music;"
    assert e["Terminal"] == "false"


def test_desktop_file_validate():
    if shutil.which("desktop-file-validate") is None:
        return  # the key test above covers the required keys
    r = subprocess.run(["desktop-file-validate", str(DESKTOP)], capture_output=True, text=True)
    assert r.returncode == 0 and not r.stdout.strip(), r.stdout


def test_icon_is_svg_with_the_theme_colors():
    root = ET.parse(ICON).getroot()
    assert root.tag.endswith("svg")
    text = ICON.read_text(encoding="utf-8").lower()
    assert "#55e69a" in text and "#101310" in text
