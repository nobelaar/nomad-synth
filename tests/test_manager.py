import gc
import threading
import weakref

import numpy as np
import pytest

from nomad.engine.engine import Engine
from nomad.engine.manager import InstrumentManager


def render(e, n=64):
    o = np.zeros(n)
    e.render(o)
    return o


@pytest.fixture
def env():
    e = Engine(48000)
    pending = []
    m = InstrumentManager(e, schedule=lambda ms, fn: pending.append(fn))
    return e, m, pending


def test_add_names_and_ids(env):
    e, m, _ = env
    s2, d2 = m.add("synth"), m.add("drums")
    assert [i.name for i in m.instances] == ["Synth 01", "Machine Drum 01", "Synth 02", "Machine Drum 02"]
    assert s2.id == 3 and d2.id == 4
    m.remove(s2)
    assert m.add("synth").name == "Synth 03"  # names and ids are never reused
    with pytest.raises(KeyError):
        m.add("sampler")


def test_two_followers_of_one_master_allowed(env):
    e, m, _ = env
    s2 = m.add("synth")
    assert m.set_source(s2, e.synth) and m.set_source(e.drums, e.synth)
    render(e)
    assert e.topology.roots == (e.synth.clock,)


def test_chain_rejected(env):
    e, m, _ = env
    s2 = m.add("synth")
    assert m.set_source(e.drums, e.synth)
    assert not m.set_source(s2, e.drums)       # drums is a follower: a chain
    assert not m.set_source(e.synth, s2)       # synth has followers: it cannot follow
    assert m.set_source(e.drums, s2)           # a follower may re-point to another Internal source
    assert e.drums.source is s2


def test_self_and_dead_source_rejected(env):
    e, m, _ = env
    s2 = m.add("synth")
    assert not m.set_source(s2, s2)
    m.remove(s2)
    assert not m.set_source(e.drums, s2)


def test_follower_play_and_bpm_ignored(env):
    e, m, _ = env
    m.set_source(e.drums, e.synth)
    assert not m.start(e.drums) and not m.set_bpm(e.drums, 99.0) and not m.stop(e.drums)
    assert m.start(e.synth) and m.set_bpm(e.synth, 128.0)
    render(e)
    assert e.synth.clock.running and e.synth.clock.bpm == 128.0 and not e.drums.clock.running


def test_remove_master_followers_keep_position_and_bpm(env):
    e, m, _ = env
    m.set_source(e.drums, e.synth)
    m.set_bpm(e.synth, 128.0)
    m.start(e.synth)
    render(e, 48000)
    pos, bpm = e.synth.clock.pos, e.synth.clock.bpm
    assert pos > 1.0
    m.remove(e.synth)
    render(e, 1)
    c = e.drums.clock
    assert c.running and c.bpm == bpm
    assert c.pos == pytest.approx(pos + 1 * bpm / (60 * 48000), abs=1e-9)  # exact continuity, then advances


def test_follower_goes_internal_with_handover(env):
    e, m, _ = env
    m.set_source(e.drums, e.synth)
    m.set_bpm(e.synth, 100.0)
    m.start(e.synth)
    render(e, 24000)
    pos = e.synth.clock.pos
    assert m.set_source(e.drums, None)
    render(e, 1)
    assert e.drums.clock.running and e.drums.clock.bpm == 100.0
    assert e.drums.clock.pos == pytest.approx(pos + 100.0 / (60 * 48000), abs=1e-9)
    assert m.set_bpm(e.drums, 90.0)  # Internal again: editable


def test_source_switch_is_immediate_next_render(env):
    e, m, _ = env
    assert len(e.topology.roots) == 2
    m.set_source(e.drums, e.synth)
    assert len(e.topology.roots) == 2          # not adopted until the audio thread runs
    render(e)
    assert e.topology.roots == (e.synth.clock,)
    m.set_source(e.drums, None)
    render(e)
    assert len(e.topology.roots) == 2


def test_removed_instance_not_dropped_in_audio_thread(env):
    e, m, pending = env
    s2 = m.add("synth")
    render(e)                                  # adopts the topology that contains s2
    old = weakref.ref(e.topology)
    m.remove(s2)
    render(e)                                  # adopts the "closing" topology
    assert old() is not None                   # the old snapshot waits in the graveyard
    m.collect()
    gc.collect()
    assert old() is None                       # the last reference died on the GUI thread
    assert len(pending) == 1                   # final drop is scheduled, not immediate
    pending.pop()()
    render(e)
    assert s2 not in e.topology.insts


def test_closed_instance_is_silenced_before_being_dropped(env):
    e, m, pending = env
    s2 = m.add("synth")
    s2.note_on(60, 1.0)
    render(e, 2048)
    m.remove(s2)
    render(e, 1024)
    assert s2.vm.active_notes() == []
    assert s2 in e.topology.insts              # still rendering its fade until the delayed drop


def test_stress_random_ops_with_real_classes():
    import random
    import time

    e = Engine(48000)
    m = InstrumentManager(e)
    stop, errs = threading.Event(), []

    def audio():
        rng = random.Random(1)
        while not stop.is_set():
            o = np.zeros(rng.choice((1, 37, 256, 512, 700, 2048)))
            try:
                e.render(o)
                topo = e.topology
                for eff in topo.eff:
                    assert any(eff is r for r in topo.roots), "follower of a clock nobody advances"
                assert np.isfinite(o).all()
            except Exception as exc:  # noqa: BLE001
                errs.append(repr(exc))
                return

    t = threading.Thread(target=audio)
    t.start()
    rng, end = random.Random(2), time.time() + 3.0
    while time.time() < end and not errs:
        op, insts = rng.random(), m.instances
        if op < 0.12 and len(insts) < 6:
            m.add(rng.choice(("synth", "drums")))
        elif op < 0.24 and len(insts) > 1:
            m.remove(rng.choice(insts))
        elif op < 0.50:
            m.set_source(rng.choice(insts), rng.choice([None] + list(insts)))
        elif op < 0.70:
            (m.start if rng.random() < 0.5 else m.stop)(rng.choice(insts))
        elif op < 0.85:
            m.set_bpm(rng.choice(insts), rng.uniform(60, 180))
        else:
            m.set_mute(rng.choice(insts), rng.random() < 0.5)
        if rng.random() < 0.05:
            m.collect()
            time.sleep(0.001)
    stop.set()
    t.join()
    assert errs == [] and e.errors == 0


def test_close_master_race_does_not_stop_or_rewind_followers(env):
    """The GUI may call remove() between Engine._drain() and inst.drain() of the SAME render."""
    e, m, _ = env
    m.set_source(e.drums, e.synth)
    m.set_bpm(e.synth, 128.0)
    m.start(e.synth)
    render(e, 48000)
    pos = e.synth.clock.pos
    real, fired = e._drain, []

    def racy():
        real()
        if not fired:
            fired.append(1)
            m.remove(e.synth)          # lands after the topology queue was drained, before inst.drain()

    e._drain = racy
    render(e, 512)                     # the race happens inside this render
    e._drain = real
    render(e, 512)                     # this one adopts the removal
    render(e, 512)
    c = e.drums.clock
    assert c.running and c.bpm == 128.0 and c.pos > pos      # still running, never rewound to 0


def test_close_master_at_a_buffer_edge_never_repeats_a_step(env):
    e, m, _ = env
    sq = e.synth.sequencer
    sq.enabled = True
    for s in range(16):
        sq.set_step(s, True, note=48 + s)
    log, real = [], sq.on_step
    sq.on_step = lambda k: (log.append(k), real(k))[1]
    m.set_bpm(e.drums, 120.0)
    m.set_source(e.synth, e.drums)
    m.start(e.drums)
    for _ in range(3):
        render(e, 6000)                # every buffer edge lands exactly on a step boundary at 120 BPM
    m.remove(e.drums)
    for _ in range(4):
        render(e, 6000)
    assert len(log) >= 6 and all(b == (a + 1) % 16 for a, b in zip(log, log[1:])), log
