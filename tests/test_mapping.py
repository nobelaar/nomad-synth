import json

import pytest

from nomad.engine.params import REGISTRY
from nomad.midi.mapping import MidiMap
from nomad.midi.midi_input import MidiEvent


def cc(control, value, port="keys", channel=0):
    return MidiEvent("cc", 0, 0.0, control, value, channel, port, 0.0)


def test_learn_binds_next_cc_to_armed_param(tmp_path):
    m = MidiMap(tmp_path / "map.json")
    m.arm("filt_cutoff")
    assert m.learning == "filt_cutoff"
    out = m.handle(cc(74, 64))
    assert out is not None
    name, value = out
    spec = REGISTRY["filt_cutoff"]
    assert name == "filt_cutoff" and spec.lo <= value <= spec.hi
    assert m.bindings[("keys", 0, 74)] == "filt_cutoff"


def test_learn_disarms_after_one_cc(tmp_path):
    m = MidiMap(tmp_path / "map.json"); m.arm("filt_cutoff")
    m.handle(cc(74, 10)); m.handle(cc(75, 10))
    assert ("keys", 0, 75) not in m.bindings and m.learning is None


def test_saved_map_reloads(tmp_path):
    p = tmp_path / "map.json"
    m = MidiMap(p); m.arm("filt_reso"); m.handle(cc(71, 0)); m.save()
    m2 = MidiMap(p); m2.load()
    assert m2.bindings[("keys", 0, 71)] == "filt_reso"
    assert json.loads(p.read_text())["version"] == 1


def test_fallback_matches_same_channel_and_cc_on_other_port(tmp_path):
    m = MidiMap(tmp_path / "map.json"); m.arm("filt_reso"); m.handle(cc(71, 0))
    out = m.handle(cc(71, 100, port="otro"))
    assert out[0] == "filt_reso"
    assert m.handle(cc(71, 100, port="otro", channel=1)) is None


def test_cc_on_unknown_control_does_nothing(tmp_path):
    assert MidiMap(tmp_path / "m.json").handle(cc(3, 10)) is None


def test_arm_unknown_raises_and_none_cancels():
    m = MidiMap()
    with pytest.raises(KeyError):
        m.arm("nope")
    m.arm("filt_reso"); m.arm(None)
    assert m.learning is None and m.handle(cc(5, 5)) is None


def test_non_cc_event_ignored_and_keeps_armed():
    m = MidiMap(); m.arm("filt_reso")
    assert m.handle(MidiEvent("note_on", note=60, velocity=1.0)) is None
    assert m.learning == "filt_reso"


def test_relearn_param_drops_old_cc_and_cc_rebinds():
    m = MidiMap()
    m.arm("filt_reso"); m.handle(cc(71, 0))
    m.arm("filt_reso"); m.handle(cc(72, 0))
    assert m.bindings == {("keys", 0, 72): "filt_reso"}
    m.arm("filt_cutoff"); m.handle(cc(72, 0))  # CC rebinds to the new param
    assert m.bindings == {("keys", 0, 72): "filt_cutoff"}


def test_exact_binding_beats_fallback():
    m = MidiMap()
    m.arm("filt_reso"); m.handle(cc(71, 0, port="a"))
    m.arm("filt_cutoff"); m.handle(cc(71, 0, port="b"))
    assert m.handle(cc(71, 0, port="a"))[0] == "filt_reso"
    assert m.handle(cc(71, 0, port="b"))[0] == "filt_cutoff"


def test_discrete_params_are_rounded_integers():
    m = MidiMap()
    m.arm("osc1_wave"); m.handle(cc(20, 0))
    assert m.handle(cc(20, 64)) == ("osc1_wave", 2.0)
    assert m.handle(cc(20, 127)) == ("osc1_wave", 3.0)
    assert m.handle(cc(20, 0)) == ("osc1_wave", 0.0)
    m.arm("polyphony"); m.handle(cc(21, 0))
    assert m.handle(cc(21, 127)) == ("polyphony", 8.0)


def test_load_missing_file_is_empty_no_error(tmp_path):
    m = MidiMap(tmp_path / "none.json"); m.load()
    assert m.bindings == {} and m.error is None


def test_load_corrupt_json_leaves_file_and_sets_error(tmp_path):
    p = tmp_path / "m.json"; p.write_text("{not json")
    m = MidiMap(p); m.load()
    assert m.bindings == {} and m.error and p.read_text() == "{not json"


def test_load_ignores_bad_entries(tmp_path):
    p = tmp_path / "m.json"
    good = {"port": "k", "channel": 1, "cc": 7, "param": "filt_reso"}
    p.write_text(json.dumps({"version": 1, "bindings": [
        good, {**good, "param": "bogus", "cc": 8}, {**good, "cc": "9"},
        {**good, "channel": True, "cc": 10}, {**good, "cc": 300}, 5, {"port": "k"}]}))
    m = MidiMap(p); m.load()
    assert m.bindings == {("k", 1, 7): "filt_reso"} and m.error is None


@pytest.mark.parametrize("content", [
    {"version": 2, "bindings": []}, {"bindings": []}, {"version": 1, "bindings": 3}, [1]])
def test_load_bad_version_or_shape_is_empty_with_error(tmp_path, content):
    p = tmp_path / "m.json"; p.write_text(json.dumps(content))
    m = MidiMap(p); m.load()
    assert m.bindings == {} and m.error


def test_save_creates_dir_atomically(tmp_path):
    p = tmp_path / "a" / "b" / "m.json"
    m = MidiMap(p); m.arm("filt_reso"); m.handle(cc(71, 0)); m.save()
    assert p.exists() and not list(p.parent.glob("*.tmp"))


def test_save_failure_keeps_old_file(tmp_path, monkeypatch):
    p = tmp_path / "m.json"
    m = MidiMap(p); m.arm("filt_reso"); m.handle(cc(71, 0)); m.save()
    before = p.read_text()
    def boom(*a): raise OSError("disk")
    monkeypatch.setattr("nomad.midi.mapping.os.replace", boom)
    with pytest.raises(OSError):
        m.save()
    assert p.read_text() == before and not list(tmp_path.glob("*.tmp"))


def test_path_none_is_noop():
    m = MidiMap(); m.arm("filt_reso"); m.handle(cc(71, 0))
    m.save(); m.load()
    assert len(m.bindings) == 1


def test_unbind():
    m = MidiMap(); m.arm("filt_reso"); m.handle(cc(71, 0))
    m.unbind("filt_reso"); m.unbind("never")
    assert m.bindings == {} and m.handle(cc(71, 5)) is None
