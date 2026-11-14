import numpy as np

from nomad.engine.instrument import SynthInstrument


def render(inst, n=512):
    inst.drain()
    b = np.zeros(n)
    for s in range(0, n, 256):
        inst.render_block(b[s:s + 256], 256)
    return b


def test_two_instruments_are_independent():
    a, b = SynthInstrument(48000), SynthInstrument(48000)
    a.note_on(60, 1.0)
    ya, yb = render(a), render(b)
    assert np.abs(ya).max() > 0 and not yb.any()
    assert a.snapshot()["active_notes"] == [60] and b.snapshot()["active_notes"] == []


def test_instrument_param_isolation():
    a, b = SynthInstrument(48000), SynthInstrument(48000)
    name = next(n for n in a.snapshot() if n.endswith("cutoff"))
    a.set_param(name, 123.0)
    assert b.snapshot()[name] != a.snapshot()[name]
