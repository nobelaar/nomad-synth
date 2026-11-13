import numpy as np
import pytest

from nomad.audio.scope import ScopeBuffer
from nomad.audio.stream import (JACK, AudioStream, OutputDevice, list_output_devices, pick_device)


def test_devices_have_indices_and_names():
    assert all(isinstance(d.index, int) and d.name for d in list_output_devices())


def test_list_never_raises_without_portaudio(monkeypatch):
    import sounddevice as sd
    def boom(*a, **k): raise sd.PortAudioError("no devices")
    monkeypatch.setattr(sd, "query_devices", boom)
    assert list_output_devices() == []


class FakeEngine:
    def __init__(self): self.calls = 0
    def render(self, out): out[:] = 0.1; self.calls += 1


class T:
    inputBufferAdcTime = 0.0; currentTime = 0.0


def test_callback_renders_and_counts():
    s = AudioStream(FakeEngine())
    out = np.zeros((256, 2), dtype=np.float32)
    s.callback(out, 256, T(), None)
    assert np.allclose(out, 0.1)


def test_late_callback_counts_as_xrun():
    s = AudioStream(FakeEngine())
    out = np.zeros((256, 2), dtype=np.float32)
    s.callback(out, 256, T(), None)
    class Late: currentTime = 0.02
    s.callback(out, 256, Late(), None)
    assert s.xruns >= 1 and s.max_late_ms > 10


def test_on_time_callbacks_are_not_xruns():
    s = AudioStream(FakeEngine())
    out = np.zeros((256, 2), dtype=np.float32)
    for k in range(4):
        t = T(); t.currentTime = k * 256 / 48000
        s.callback(out, 256, t, None)
    assert s.xruns == 0


def test_status_underflow_counts():
    s = AudioStream(FakeEngine())
    class St: output_underflow = True
    s.callback(np.zeros((256, 2), dtype=np.float32), 256, T(), St())
    assert s.xruns == 1


def test_engine_error_yields_silence_and_counts():
    class Bad:
        def render(self, out): raise ValueError("x")
    s = AudioStream(Bad())
    out = np.ones((256, 2), dtype=np.float32)
    s.callback(out, 256, T(), None)
    assert s.errors == 1 and not out.any()


def test_close_stop_idempotent_and_safe_before_start():
    s = AudioStream(FakeEngine()); s.stop(); s.close(); s.close()


def test_callback_feeds_scope():
    sc = ScopeBuffer(48000, seconds=0.01)
    s = AudioStream(FakeEngine(), scope=sc)
    s.callback(np.zeros((256, 2), dtype=np.float32), 256, T(), None)
    assert np.allclose(sc.window(256), 0.1)


def D(i, api, ch=2): return OutputDevice(i, f"d{i}", api, 48000.0, ch)


def test_pick_prefers_jack_then_default_then_error():
    devs = [D(0, "ALSA"), D(5, "ALSA"), D(6, JACK), D(7, JACK, 1)]
    assert pick_device(devs, 0) == 6
    assert pick_device([D(0, "ALSA"), D(5, "ALSA")], 5) == 5
    assert pick_device([D(7, JACK, 1), D(0, "ALSA")], 0) == 0
    with pytest.raises(RuntimeError, match="No audio output"):
        pick_device([], None)
    with pytest.raises(RuntimeError):
        pick_device([D(0, "ALSA")], -1)


@pytest.mark.skipif(not list_output_devices(), reason="no audio output device")
def test_real_stream_runs_silently():
    import time
    from nomad.engine.engine import Engine
    e = Engine(); e.set_param("master_volume", 0.0); e.note_on(60, 0.8)
    s = AudioStream(e)
    try:
        s.start(); s.start()
        time.sleep(0.3)
        s.stop()
        n = s.callbacks
        s.start()  # restart after stop must produce audio again
        time.sleep(0.3)
        assert s.callbacks > n
    finally:
        s.close()
    print(f"callbacks={s.callbacks} xruns={s.xruns} errors={s.errors} max_late_ms={s.max_late_ms:.2f}")
    assert s.callbacks > 0 and s.errors == 0 and s.xruns <= 1
