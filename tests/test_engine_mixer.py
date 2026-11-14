import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.engine.topology import Topology


def render(e, *sizes):
    parts = []
    for n in sizes:
        o = np.zeros(n)
        e.render(o)
        parts.append(o)
    return np.concatenate(parts)


def synth_only():
    e = Engine(48000)
    e.note_on(60, 0.8)
    return e


def test_default_level_is_bit_identical_to_before():
    gold = np.load("tests/data/engine_golden.npy")
    e = Engine(48000)
    e.drums.set_value("bpm", 120.0)
    e.drums.start()
    e.note_on(60, 0.8)
    e.note_on(67, 0.6)
    out = []
    for n in (512, 512, 300, 700, 1024, 2048, 37, 1000, 1500, 1000):
        o = np.zeros(n)
        e.render(o)
        out.append(o)
        if len(out) == 4:
            e.note_off(60)
            e.note_on(64, 1.0)
    assert np.abs(np.concatenate(out) - gold).max() < 1e-12


def test_mute_ramps_without_click_and_ends_silent():
    ref, e = synth_only(), synth_only()
    render(ref, 2048), render(e, 2048)
    e.synth.set_mute(True)
    y, r = render(e, 600), render(ref, 600)
    assert np.abs(np.diff(y[:256])).max() <= np.abs(np.diff(r[:256])).max() * 1.05
    assert np.abs(y[300:]).max() == 0.0


def test_unmute_restores_level_with_a_ramp():
    e = synth_only()
    render(e, 2048)
    e.synth.set_mute(True)
    render(e, 600)
    e.synth.set_mute(False)
    y = render(e, 600)
    assert abs(y[0]) < 0.05 and np.abs(y[300:]).max() > 0.01


def test_level_scales_only_that_instance():
    a, b = synth_only(), synth_only()
    b.synth.set_level(0.5)
    ya, yb = render(a, 2048), render(b, 2048)
    assert 0 < np.abs(yb).max() < np.abs(ya).max()
    b.synth.set_level(float("nan"))
    render(b, 512)
    assert b.synth._level == 1.0


def test_mute_keeps_clock_running():
    e = Engine(48000)
    e.drums.set_value("bpm", 120.0)
    e.drums.set_mute(True)
    e.drums.start()
    render(e, 6000)
    assert e.drums.clock.pos == pytest.approx(0.25) and e.drums.position == 1


def test_master_volume_is_global_and_instances_have_no_master():
    e = Engine(48000)
    e.drums.start()
    e.note_on(60, 1.0)
    e.set_param("master_volume", 0.0)
    y = render(e, 48000)
    assert e.snapshot()["master_volume"] == 0.0 and np.abs(y[-256:]).max() < 1e-4
    with pytest.raises(KeyError):
        e.synth.set_param("master_volume", 0.5)


def test_each_root_clock_advances_once_per_chunk():
    e = Engine(48000)
    counts = {}
    for name, inst in (("synth", e.synth), ("drums", e.drums)):
        real = inst.clock.advance
        counts[name] = 0

        def wrap(n, real=real, name=name):
            counts[name] += 1
            real(n)

        inst.clock.advance = wrap
    e.drums.start()
    render(e, 512)
    assert counts["synth"] == counts["drums"] >= 1


def test_gui_calls_only_enqueue():
    e = Engine(48000)
    e.drums.start()
    e.synth.set_mute(True)
    e.set_param("master_volume", 0.2)
    assert not e.drums.clock.running and not e.synth._muted and e._master_p != 0.2
    render(e, 64)
    assert e.drums.clock.running and e.synth._muted


def test_panic_goes_through_queue():
    e = Engine(48000)
    e.play()
    render(e, 64)
    e.panic()
    assert e.drums.clock.running
    render(e, 64)
    assert not e.drums.clock.running


def test_publish_adopts_topology_returns_old_and_hands_over_clocks():
    e = Engine(48000)
    old = e.topology
    e.drums.clock.bpm, e.drums.clock.pos, e.drums.clock.running = 133.0, 3.25, True
    e.publish(Topology([e.synth]), handovers=[(e.synth.clock, e.drums.clock)])
    assert e.topology is old
    render(e, 64)
    assert e.topology.insts == (e.synth,)
    assert (e.synth.clock.bpm, e.synth.clock.running) == (133.0, True)
    assert e.synth.clock.pos == pytest.approx(3.25, abs=0.01)
    assert e.collect() == [old] and e.collect() == []
