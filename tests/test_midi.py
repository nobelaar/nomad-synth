import time

import mido
import pytest

from nomad.midi.midi_input import MidiEvent, MidiInput, MidiRouter, parse_message


def test_note_on_with_velocity_zero_is_note_off():
    e = parse_message(mido.Message("note_on", note=60, velocity=0), "p")
    assert e.kind == "note_off" and e.note == 60

def test_velocity_is_normalized():
    e = parse_message(mido.Message("note_on", note=60, velocity=127), "p")
    assert e.kind == "note_on" and e.velocity == 1.0

def test_cc_parsed():
    e = parse_message(mido.Message("control_change", control=64, value=127), "p")
    assert e.kind == "cc" and e.cc == 64 and e.value == 127

def test_pitch_bend_is_bipolar():
    up = parse_message(mido.Message("pitchwheel", pitch=8191), "p")
    down = parse_message(mido.Message("pitchwheel", pitch=-8192), "p")
    assert up.bend > 0.99 and down.bend < -0.99

def test_clock_and_others_are_ignored():
    assert parse_message(mido.Message("clock"), "p") is None


class FakeEngine:
    def __init__(self):
        self.notes_on = []; self.notes_off = []; self.controls = []; self.panics = 0
    def note_on(self, n, v): self.notes_on.append(n)
    def note_off(self, n): self.notes_off.append(n)
    def set_control(self, name, value): self.controls.append((name, value))
    def panic(self): self.panics += 1

def ev(kind, note=0, cc=0, value=0, bend=0.0, ch=0):
    return MidiEvent(kind, note, 0.8, cc, value, ch, "p", bend)

def setup():
    e = FakeEngine()
    return e, MidiRouter(e)

def test_router_plays_notes():
    e, r = setup()
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_on == [60] and e.notes_off == [60]

def test_sustain_holds_notes_until_pedal_release():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_off == []
    r.handle(ev("cc", cc=64, value=0))
    assert e.notes_off == [60]

def test_bend_and_modwheel_go_to_engine():
    e, r = setup()
    r.handle(ev("bend", bend=1.0)); r.handle(ev("cc", cc=1, value=127))
    assert ("bend", 1.0) in e.controls
    assert any(n == "modwheel" and v > 0.99 for n, v in e.controls)

def test_modwheel_at_rest_is_zero():
    e, r = setup()
    r.handle(ev("cc", cc=1, value=0))
    assert e.controls == [("modwheel", 0.0)]

def test_open_missing_port_raises_cleanly():
    m = MidiInput(lambda e: None)
    with pytest.raises(RuntimeError):
        m.open("no-existe")
    m.close()


# ---- sustain edge cases ----------------------------------------------------
def test_retapped_note_cancels_pending_off():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    r.handle(ev("note_on", note=60))
    r.handle(ev("cc", cc=64, value=0))
    assert e.notes_off == [] and e.notes_on == [60, 60]

def test_pedal_up_releases_each_pending_once():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=127))
    for n in (60, 64):
        r.handle(ev("note_on", note=n)); r.handle(ev("note_off", note=n))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    r.handle(ev("cc", cc=64, value=0)); r.handle(ev("cc", cc=64, value=0))
    assert sorted(e.notes_off) == [60, 64]

def test_note_off_with_pedal_up_passes_through():
    e, r = setup()
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_off == [60]

def test_pedal_threshold_is_64():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=63))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_off == [60]
    r.handle(ev("cc", cc=64, value=64))
    r.handle(ev("note_on", note=61)); r.handle(ev("note_off", note=61))
    assert e.notes_off == [60]

def test_pedal_state_survives_all_sound_off():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("cc", cc=120))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_off == []

def test_all_sound_off_panics_and_drops_pending():
    e, r = setup()
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    r.handle(ev("cc", cc=120))
    r.handle(ev("cc", cc=64, value=0))
    assert e.panics == 1 and e.notes_off == []

def test_all_notes_off_releases_held_and_pending():
    e, r = setup()
    r.handle(ev("note_on", note=60)); r.handle(ev("note_on", note=62))
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("note_off", note=62))
    r.handle(ev("cc", cc=123))
    assert sorted(e.notes_off) == [60, 62] and e.panics == 0
    r.handle(ev("cc", cc=123))
    assert len(e.notes_off) == 2

def test_other_ccs_go_to_on_cc():
    got = []
    e = FakeEngine(); r = MidiRouter(e, on_cc=lambda c, v, ch: got.append((c, v, ch)))
    r.handle(ev("cc", cc=74, value=33, ch=5)); r.handle(ev("cc", cc=1, value=5))
    assert got == [(74, 33, 5)]

def test_cc_without_on_cc_is_ignored():
    setup()[1].handle(ev("cc", cc=74, value=1))


# ---- parse robustness --------------------------------------------------------
def test_parse_sysex_aftertouch_and_junk_return_none():
    assert parse_message(mido.Message("sysex", data=[1, 2]), "p") is None
    assert parse_message(mido.Message("aftertouch", value=5), "p") is None
    assert parse_message(object(), "p") is None
    class Bad:  # note_on missing attrs
        type = "note_on"
    assert parse_message(Bad(), "p") is None

def test_velocity_clamped_and_channel_kept():
    class M:
        type = "note_on"; note = 60; velocity = 500; channel = 9
    e = parse_message(M(), "p")
    assert e.velocity == 1.0 and e.channel == 9


# ---- MidiInput ----------------------------------------------------------------
def test_list_ports_never_raises(monkeypatch):
    def boom(): raise OSError("no backend")
    monkeypatch.setattr(mido, "get_input_names", boom)
    assert MidiInput(lambda e: None).list_ports() == []

def test_open_none_without_ports_raises(monkeypatch):
    monkeypatch.setattr(mido, "get_input_names", lambda: [])
    with pytest.raises(RuntimeError, match="no MIDI"):
        MidiInput(lambda e: None).open()

def test_missing_port_error_lists_available(monkeypatch):
    monkeypatch.setattr(mido, "get_input_names", lambda: ["A", "B"])
    with pytest.raises(RuntimeError, match="'A', 'B'"):
        MidiInput(lambda e: None).open("C")

def test_handler_exception_is_counted_not_raised():
    def bad(e): raise ValueError
    m = MidiInput(bad)
    m._dispatch(mido.Message("note_on", note=60, velocity=9), "p")
    assert m.errors == 1

def test_close_idempotent_and_is_open_false():
    m = MidiInput(lambda e: None)
    assert not m.is_open
    m.close(); m.close()

def test_virtual_port_roundtrip():
    try:
        mido.set_backend("mido.backends.rtmidi")
        out = mido.open_output("nomad-test", virtual=True)
    except Exception as exc:
        pytest.skip(f"virtual MIDI ports unavailable: {exc}")
    got = []
    m = MidiInput(got.append)
    try:
        name = next((n for n in m.list_ports() if "nomad-test" in n), None)
        if name is None:
            pytest.skip("virtual port not listed")
        m.open(name)
        assert m.is_open
        out.send(mido.Message("note_on", note=60, velocity=100))
        out.send(mido.Message("note_off", note=60))
        out.send(mido.Message("control_change", control=1, value=64))
        deadline = time.time() + 1
        while len(got) < 3 and time.time() < deadline:
            time.sleep(0.01)
        assert [g.kind for g in got] == ["note_on", "note_off", "cc"]
        m.open(name)  # reopening closes the previous one
        assert m.is_open
    finally:
        m.close(); out.close()
    assert not m.is_open

def test_reset_releases_held_and_pending_and_clears_pedal():
    e, r = setup()
    r.handle(ev("note_on", note=60))
    r.handle(ev("cc", cc=64, value=127))
    r.handle(ev("note_on", note=61)); r.handle(ev("note_off", note=61))
    r.reset()
    assert sorted(e.notes_off) == [60, 61]
    r.handle(ev("note_on", note=62)); r.handle(ev("note_off", note=62))
    assert 62 in e.notes_off  # pedal no longer latched
    r.reset(); r.reset()  # idempotent
