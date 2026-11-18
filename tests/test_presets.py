import json
import tkinter as tk

import numpy as np
import pytest

from nomad import presets
from nomad.engine.engine import Engine
from nomad.gui.app import build_app
from nomad.presets import FACTORY, PresetStore, apply, capture, check


def test_capture_apply_roundtrip_synth_with_chord():
    e = Engine(48000)
    s, sq = e.synth, e.synth.sequencer
    s.set_param("filt_cutoff", 777.0)
    s.set_param("mod2_src", 1)
    sq.set_step(3, True, note=50, vel=127)
    sq.add_note(3, 55)
    sq.grid.division, sq.grid.length, sq.gate = 4.0, 4, 0.9
    snap = json.loads(json.dumps(capture(s, "x")))  # through JSON, as a user file would be
    e2 = Engine(48000)
    apply(e2.synth, snap)
    assert capture(e2.synth, "x") == snap and e2.synth.sequencer.chord(3) == [50, 55]


def test_capture_apply_roundtrip_drums():
    e = Engine(48000)
    e.drums.set_value("kick_decay", 0.9)
    e.drums.set_step("tom", 5, 2)
    snap = capture(e.drums, "d")
    e2 = Engine(48000)
    apply(e2.drums, json.loads(json.dumps(snap)))
    assert e2.drums.values["kick_decay"] == pytest.approx(0.9) and e2.drums.steps[3, 5] == 2


def test_tempo_mix_and_seq_enable_are_not_in_a_preset():
    e = Engine(48000)
    e.synth.sequencer.enabled = True
    e.drums.set_value("bpm", 150)
    apply(e.synth, FACTORY["synth"][0])
    apply(e.drums, FACTORY["drums"][0])
    assert e.synth.sequencer.enabled and e.drums.values["bpm"] == 150
    assert "bpm" not in capture(e.drums)["params"] and "master_volume" not in capture(e.synth)["params"]


@pytest.mark.parametrize("kind", ["synth", "drums"])
def test_every_factory_preset_applies_and_sounds(kind):
    for p in FACTORY[kind]:
        e = Engine(48000)
        inst = e.synth if kind == "synth" else e.drums
        apply(inst, p)
        if kind == "synth":
            inst.sequencer.enabled = True
            e.seq_clock.start()
        else:
            e.drums.start()
        out = np.zeros(48000 * 3)
        e.render(out)
        assert np.isfinite(out).all() and np.abs(out).max() > 0.01, p["name"]


def test_bad_presets_are_rejected_and_change_nothing():
    e = Engine(48000)
    good = capture(e.synth, "g")
    before = capture(e.synth)
    bad = [
        {}, "x", {**good, "kind": "drums"}, {**good, "params": {"filt_cutoff": "loud"}},
        {**good, "seq": {**good["seq"], "on": [1] * 15}},
        {**good, "seq": {**good["seq"], "note": [300] * 16}},
        {**good, "seq": {**good["seq"], "length": 0}},
    ]
    for b in bad:
        with pytest.raises(ValueError):
            apply(e.synth, b)
    assert capture(e.synth) == before


def test_unknown_params_are_skipped_and_values_clamped():
    e = Engine(48000)
    d = capture(e.synth, "g")
    d["params"].update({"nope": 1.0, "filt_cutoff": 1e12})
    apply(e.synth, d)
    assert e.synth._values["filt_cutoff"] == 18000.0


def test_store_save_list_get_and_sanitise(tmp_path):
    st, e = PresetStore(tmp_path), Engine(48000)
    name = st.save("synth", "  mi/pad ../x  ", capture(e.synth))
    assert name == "mipad x"
    assert (name, "user") in st.list("synth") and st.get("synth", name)["name"] == name
    assert [n for n, o in st.list("synth") if o == "factory"] == [p["name"] for p in FACTORY["synth"]]
    assert list(tmp_path.rglob("*.tmp")) == [] and all(p.parent.parent == tmp_path for p in tmp_path.rglob("*.json"))
    with pytest.raises(ValueError):
        st.save("synth", "bajo", capture(e.synth))  # factory name (case-insensitive)
    with pytest.raises(ValueError):
        st.save("synth", "///", capture(e.synth))
    (tmp_path / "synth" / "junk.json").write_text("{nope")
    assert (name, "user") in st.list("synth")  # corrupt file is skipped, not fatal


def test_store_without_dir_is_factory_only():
    st = PresetStore(None)
    assert not st.can_save and all(o == "factory" for _, o in st.list("drums"))
    with pytest.raises(ValueError):
        st.save("drums", "mine", capture(Engine(48000).drums))


@pytest.fixture
def app(tmp_path):
    try:
        a = build_app(Engine(48000), presets=PresetStore(tmp_path))
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    a.root.update()
    yield a
    a.close()


def test_strip_loads_preset_and_refreshes_widgets(app):
    s = app.engine.synth
    app.strips[s.id].load_preset("Bajo")
    app.root.update()
    assert s._values["polyphony"] == 1 and app.panels[s.id].widgets["polyphony"].index == 0
    assert app.panels[s.id].seq_view.len_btn["text"] == "LEN 16"
    app.strips[s.id].load_preset("Pad de acordes")
    assert s.sequencer.grid.length == 4 and app.panels[s.id].seq_view.len_btn["text"] == "LEN 4"
    d = app.engine.drums
    app.strips[d.id].load_preset("Minimal")
    app.root.update()
    assert d.steps[0, 8] == 1 and app.strips[d.id].preset_btn["text"].startswith("Minimal")


def test_strip_menu_lists_factory_then_user(app):
    st = app.strips[app.engine.synth.id]
    app.presets.save("synth", "Mio", capture(app.engine.synth))
    st._fill_presets()
    m = st.preset_menu
    labels = [m.entrycget(i, "label") for i in range(m.index("end") + 1) if m.type(i) == "command"]
    assert labels == [p["name"] for p in FACTORY["synth"]] + ["Mio"]
