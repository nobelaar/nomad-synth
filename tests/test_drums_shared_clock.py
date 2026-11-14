import numpy as np

from nomad.engine.drums import DrumMachine
from nomad.engine.transport import Transport

SR = 48000.0


def test_dm_owns_transport_by_default():
    assert DrumMachine(SR).transport is not None


def test_two_drum_machines_share_one_transport():
    t = Transport(SR)
    a, b = DrumMachine(SR, transport=t), DrumMachine(SR, transport=t)
    for d in (a, b):
        d.clear_pattern()
        d.set_step("chat", 0, 1)
    a._ensure(6000), b._ensure(6000)
    t.start()
    oa, ob = np.zeros(6000), np.zeros(6000)
    t.run(6000, lambda i, c: (a.render_chunk(oa, i, c), b.render_chunk(ob, i, c)),
          lambda p: (a.on_step(p), b.on_step(p)))
    assert np.abs(oa).max() > 0 and np.array_equal(oa, ob)
