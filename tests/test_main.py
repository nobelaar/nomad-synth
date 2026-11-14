import signal
import threading
import time
import tkinter as tk
import wave

import numpy as np
import pytest

from nomad.main import build_runtime, default_config_dir


def rt_for(tmp_path, *argv):
    return build_runtime([*argv, "--config-dir", str(tmp_path)])


class FakeStream:
    def __init__(self):
        self.started = self.closed = False
        self.callbacks = self.xruns = self.errors = 0
        self.active_device = None
        self.scope = None

    def start(self):
        self.started = True

    def close(self):
        self.closed = True


def test_runtime_builds_without_midi(tmp_path):
    rt = rt_for(tmp_path, "--no-midi", "--dry-run")
    assert rt.engine is not None and rt.stream is None and rt.app is None


def test_close_with_notes_held_is_clean(tmp_path):
    rt = rt_for(tmp_path, "--no-midi", "--dry-run")
    for n in range(60, 68):
        rt.engine.note_on(n, 1.0)
    rt.close(); rt.close()  # idempotent
    assert rt.closed


def test_render_writes_valid_wav(tmp_path):
    from nomad.render import render_demo
    p = tmp_path / "out.wav"
    render_demo("chord", p, seconds=1.0)
    with wave.open(str(p)) as w:
        assert w.getnchannels() == 1 and w.getframerate() == 48000
        assert w.getnframes() > 40000


def test_args_parsed(tmp_path):
    rt = rt_for(tmp_path, "--no-midi", "--dry-run", "--device", "3", "--demo-note", "64", "--volume", "0.12")
    a = rt.args
    assert a.no_midi and a.dry_run and a.device == 3 and a.demo_note == 64 and a.volume == 0.12
    assert rt.engine.snapshot()["master_volume"] == pytest.approx(0.12)
    rt.start()  # dry-run demo note goes straight to the engine
    rt.engine.render(np.zeros(256, dtype=np.float32))
    assert rt.engine.vm.active_notes() == [64]
    rt.close()


@pytest.mark.parametrize("bad", [["--bogus"], ["--device", "x"], ["--volume", "2"], ["--demo-note", "200"]])
def test_bad_args_exit_with_usage(bad, capsys):
    with pytest.raises(SystemExit) as e:
        build_runtime(bad)
    assert e.value.code == 2 and "usage" in capsys.readouterr().err


def test_config_dir_default_respects_xdg(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    assert default_config_dir() == tmp_path / "nomad-synth"
    rt = build_runtime(["--dry-run"])
    assert rt.midi_map.path == tmp_path / "nomad-synth" / "midi-map.json"


def test_dry_run_opens_nothing(tmp_path, monkeypatch):
    import nomad.main as m
    monkeypatch.setattr(m.MidiInput, "open", lambda *a, **k: pytest.fail("opened MIDI"))
    rt = rt_for(tmp_path, "--dry-run", "--demo-note", "60")
    before = threading.active_count()
    rt.start()
    assert rt.app is None and rt.stream is None and threading.active_count() == before
    rt.close()


@pytest.fixture
def gui_rt(tmp_path, monkeypatch):
    try:
        tk.Tk().destroy()
    except tk.TclError as e:
        pytest.skip(str(e))
    rt = rt_for(tmp_path, "--no-midi", "--demo-note", "60", "--volume", "0.1")
    rt.stream = FakeStream()
    yield rt
    rt.close()


def test_gui_start_close_via_window_protocol(gui_rt):
    rt = gui_rt
    rt.start()
    assert rt.stream.started and rt.app is not None
    assert rt.app.midi_btn.cget("text") == "MIDI: none"  # no port is not an error
    rt.app.root.update()
    rt.app.root.tk.eval(rt.app.root.protocol("WM_DELETE_WINDOW"))  # what the window manager invokes
    assert rt.closed and rt.stream.closed


def test_close_steps_are_independent(gui_rt, capsys):
    rt = gui_rt
    rt.start()
    rt.midi_in.close = lambda: (_ for _ in ()).throw(RuntimeError("boom"))
    rt.close()
    assert rt.stream.closed and rt.app._closed
    assert "boom" in capsys.readouterr().err


def test_sigint_closes_cleanly(gui_rt):
    rt = gui_rt
    old = signal.getsignal(signal.SIGINT)
    rt.start()
    rt.app.root.after(200, lambda: signal.raise_signal(signal.SIGINT))
    rt.app.root.after(3000, rt.close)  # safety net: would fail the assert below
    t0 = time.time()
    rt.app.root.mainloop()
    assert rt.closed and time.time() - t0 < 2.5
    assert signal.getsignal(signal.SIGINT) is old  # handler restored


def test_demo_note_plays_after_startup(gui_rt):
    rt = gui_rt
    rt.start()
    rt.app.root.after(900, rt.app.root.quit)
    rt.app.root.mainloop()
    buf = np.zeros(2048, dtype=np.float32)
    rt.engine.render(buf)
    assert 60 in rt.engine.vm.active_notes() and np.abs(buf).max() > 0


def test_midi_port_change_resets_router(tmp_path, monkeypatch):
    rt = rt_for(tmp_path, "--dry-run")
    log = []
    monkeypatch.setattr(rt.midi_in, "close", lambda: log.append("close"))
    monkeypatch.setattr(rt.midi_in, "open", lambda n=None: log.append(("open", n)))
    monkeypatch.setattr(rt.router, "reset", lambda: log.append("reset"))
    rt._open_midi("X")
    assert log == ["close", "reset", ("open", "X")]


def test_midi_open_error_goes_to_status(gui_rt, monkeypatch):
    rt = gui_rt
    rt.start()
    def bad(n=None): raise RuntimeError("cannot open MIDI port 'X'")
    monkeypatch.setattr(rt.midi_in, "open", bad)
    rt._open_midi("X")
    assert "cannot open" in rt.app.status.get()


def test_learned_cc_reaches_engine_and_gui(gui_rt):
    from nomad.midi.midi_input import MidiEvent
    rt = gui_rt
    rt.start()
    rt.midi_map.arm("filt_cutoff")
    rt.router.handle(MidiEvent("cc", cc=21, value=127, channel=0))  # learn
    assert rt.engine.snapshot()["filt_cutoff"] == pytest.approx(18000.0, rel=0.01)
    rt.router.handle(MidiEvent("cc", cc=21, value=0, channel=0))
    assert rt.engine.snapshot()["filt_cutoff"] == pytest.approx(20.0, rel=0.01)


def test_virtual_midi_port_shutdown_leaves_no_threads(tmp_path):
    import mido
    try:
        mido.set_backend("mido.backends.rtmidi")
        out = mido.open_output("nomad-main-test", virtual=True)
    except Exception as exc:
        pytest.skip(f"virtual MIDI unavailable: {exc}")
    rt = rt_for(tmp_path, "--dry-run")
    before = {t.ident for t in threading.enumerate()}
    try:
        name = next((n for n in rt.midi_in.list_ports() if "nomad-main-test" in n), None)
        if name is None:
            pytest.skip("virtual port not listed")
        rt._open_midi(name)
        out.send(mido.Message("note_on", note=61, velocity=100))
        t0 = time.time()
        while 61 not in rt.engine.snapshot().get("active_notes", []) and time.time() - t0 < 1:
            rt.engine.render(np.zeros(256, dtype=np.float32)); time.sleep(0.01)
        assert 61 in rt.engine.vm.active_notes()
        rt.close()
    finally:
        out.close()
    time.sleep(0.2)
    assert {t.ident for t in threading.enumerate()} <= before


def test_render_demos_and_errors(tmp_path):
    from nomad.render import DEMOS, main, render_demo
    for name in DEMOS:
        p = render_demo(name, tmp_path / f"{name}.wav", seconds=1.0)
        with wave.open(str(p)) as w:
            x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
        assert np.abs(x).max() > 500, name  # not silent
        assert np.abs(x[-2000:]).max() < np.abs(x).max()  # tail decays
    with pytest.raises(ValueError, match="unknown demo"):
        render_demo("nope", tmp_path / "x.wav")
    assert main(["--demo", "nope", str(tmp_path / "x.wav")]) == 2
    assert main(["--demo", "poly", str(tmp_path / "p.wav"), "--seconds", "0.5"]) == 0


def test_midi_through_is_listed_last():
    ports = ["Midi Through:Midi Through Port-0 14:0", "USB Keyboard 20:0"]
    assert sorted(ports, key=lambda p: "Midi Through" in p)[0] == "USB Keyboard 20:0"


def test_warm_up_does_not_touch_the_real_engine():
    from nomad.main import _warm_up
    rt = build_runtime(["--no-midi", "--dry-run"])
    _warm_up(48000, blocks=2)
    assert rt.engine.vm.active_notes() == []
