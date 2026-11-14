import numpy as np

from nomad.engine.engine import Engine


def make():
    e = Engine(48000)
    e.drums.clear_pattern()
    e.drums.set_step("chat", 0, 1)
    e.drums.set_value("bpm", 120.0)
    return e


def render(e, sizes):
    parts = []
    for n in sizes:
        o = np.zeros(n)
        e.render(o)
        parts.append(o)
    return np.concatenate(parts)


def test_drums_and_transport_start_together():
    e = make()
    e.drums.toggle()
    assert not e.clock.running  # a command: applied by the next render
    y = render(e, [37, 100])
    assert e.clock.running and y[0] != 0.0


def test_render_sizes_not_multiple_of_256_keep_step_timing():
    a, b = make(), make()
    a.play(), b.play()
    ya = render(a, [1, 37, 700, 5262])
    yb = render(b, [6000])
    assert a.drums.position == b.drums.position == 1
    assert np.allclose(ya, yb, atol=1e-9)


def test_panic_stops_transport_and_drums():
    e = make()
    e.play()
    render(e, [512])
    e.panic()
    assert e.clock.running  # queued, not applied yet
    y = render(e, [4096])
    assert not e.clock.running and not e.drums.playing
    assert np.abs(y[-256:]).max() == 0.0


def test_drums_and_sequencer_play_independently():
    e = make()
    e.synth.sequencer.enabled = True
    e.synth.sequencer.set_step(0, True, note=64)
    e.seq_clock.start()
    render(e, [100])
    assert not e.drums.playing and e.vm.active_notes() == [64]
    e.seq_clock.stop()
    e.drums.toggle()
    render(e, [100])
    assert e.drums.playing and not e.seq_clock.running and e.vm.active_notes() == []
