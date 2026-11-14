import threading

import numpy as np

from nomad.audio.scope import ScopeBuffer


def test_window_returns_latest_samples_in_order():
    s = ScopeBuffer(48000, seconds=0.01)   # 480 samples
    s.write(np.arange(100, dtype=float))
    s.write(np.arange(100, 200, dtype=float))
    assert np.array_equal(s.window(50), np.arange(150, 200, dtype=float))


def test_wraparound_is_ordered():
    s = ScopeBuffer(48000, seconds=0.01)
    s.write(np.arange(400, dtype=float))
    s.write(np.arange(400, 560, dtype=float))
    assert np.array_equal(s.window(160), np.arange(400, 560, dtype=float))


def test_write_does_not_allocate(monkeypatch):
    s = ScopeBuffer(48000, seconds=0.01)
    blk = np.zeros(256)
    s.write(blk)
    calls = {"n": 0}
    real = np.empty
    monkeypatch.setattr(np, "empty", lambda *a, **k: calls.update(n=calls["n"] + 1) or real(*a, **k))
    s.write(blk)
    assert calls["n"] == 0


def test_window_larger_than_written_or_capacity_returns_what_exists():
    s = ScopeBuffer(48000, seconds=0.01)
    assert len(s.window(10)) == 0
    s.write(np.arange(30, dtype=np.float32))
    assert np.array_equal(s.window(100), np.arange(30, dtype=np.float32))
    s.write(np.zeros(1000, dtype=np.float32))
    assert len(s.window(10_000)) == 480


def test_block_larger_than_capacity_keeps_latest():
    s = ScopeBuffer(48000, seconds=0.01)
    s.write(np.arange(1000, dtype=np.float32))
    assert np.array_equal(s.window(480), np.arange(520, 1000, dtype=np.float32))
    s.write(np.arange(1000, 1010, dtype=np.float32))
    assert np.array_equal(s.window(10), np.arange(1000, 1010, dtype=np.float32))


def test_nan_input_does_not_crash():
    s = ScopeBuffer(48000, seconds=0.01)
    s.write(np.full(64, np.nan, dtype=np.float32))
    assert np.isnan(s.window(64)).all()


def test_concurrent_window_never_raises():
    s = ScopeBuffer(48000, seconds=0.01)
    stop = threading.Event()
    errs = []

    def reader():
        try:
            while not stop.is_set():
                s.window(300)
        except Exception as e:  # pragma: no cover
            errs.append(e)

    t = threading.Thread(target=reader); t.start()
    blk = np.ones(256, dtype=np.float32)
    for _ in range(3000):
        s.write(blk)
    stop.set(); t.join()
    assert not errs
