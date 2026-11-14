"""Real-stream validation for instances: five instruments sounding, then add/close/switch/mute churn.

Usage: .venv/bin/python tools/stream_stress.py [--seconds N] [--instances K] [--held V] [--no-churn] [--gui]
Exits non-zero if the audio callback raised or more than one xrun happened.
``--gui`` also builds the Tk app and creates tabs while the stream plays, reporting the panel
build time (Tk can hold the GIL) and the xruns it caused. Needs a display.
"""
from __future__ import annotations

import argparse
import random
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from nomad.audio.stream import AudioStream  # noqa: E402
from nomad.engine.engine import Engine  # noqa: E402
from nomad.engine.manager import InstrumentManager  # noqa: E402


def populate(engine, manager, instances: int, held: int = 8, add=None) -> None:
    """Synth 01 = clock master with sequencer; extra Synths run their own sequence; drums follow Synth 01."""
    add = add or manager.add
    synths = [engine.synth] + [add("synth") for _ in range(max(0, (instances + 1) // 2 - 1))]
    drums = [engine.drums] + [add("drums") for _ in range(max(0, instances - len(synths) - 1))]
    for k, s in enumerate(synths):
        sq = s.sequencer
        sq.enabled = True
        for step in range(16):
            sq.set_step(step, step % 2 == 0 or k == 0, note=48 + 5 * k + (step * 3) % 12)
        manager.set_bpm(s, 120.0 + 4 * k)
        manager.start(s)
    for d in drums:
        manager.set_source(d, synths[0])
    for n in (36, 43, 48, 55, 60, 64, 67, 72)[:held]:  # held voices on the last synth (8 = worst-case polyphony)
        synths[-1].note_on(n, 0.7)


def churn(manager, stop: threading.Event, seed: int = 3) -> int:
    rng, ops = random.Random(seed), 0
    while not stop.is_set():
        insts = manager.instances
        op = rng.random()
        if op < 0.18 and len(insts) < 8:
            manager.add(rng.choice(("synth", "drums")))
        elif op < 0.30 and len(insts) > 2:
            manager.remove(rng.choice(insts))
        elif op < 0.55:
            manager.set_source(rng.choice(insts), rng.choice([None] + list(insts)))
        elif op < 0.70:
            manager.set_mute(rng.choice(insts), rng.random() < 0.5)
        elif op < 0.85:
            manager.set_bpm(rng.choice(insts), rng.uniform(80, 160))
        else:
            (manager.start if rng.random() < 0.6 else manager.stop)(rng.choice(insts))
        manager.collect()
        ops += 1
        time.sleep(0.05)
    return ops


def run(seconds: float = 10.0, instances: int = 5, with_churn: bool = True, held: int = 8) -> dict:
    engine = Engine(48000)
    manager = InstrumentManager(engine)
    populate(engine, manager, instances, held)
    stream = AudioStream(engine)
    stream.start()
    stop, ops = threading.Event(), [0]
    t = None
    if with_churn:
        t = threading.Thread(target=lambda: ops.__setitem__(0, churn(manager, stop)))
        t.start()
    time.sleep(seconds)
    stop.set()
    if t:
        t.join()
    stream.stop()
    out = {"instances": len(manager.instances), "churn_ops": ops[0], "callbacks": stream.callbacks,
           "xruns": stream.xruns, "errors": stream.errors, "engine_errors": engine.errors,
           "max_late_ms": round(stream.max_late_ms, 2)}
    stream.close()
    return out


def run_gui(seconds: float = 8.0) -> dict:
    from nomad.gui.app import build_app

    engine = Engine(48000)
    stream = AudioStream(engine)
    app = build_app(engine, stream)
    app.root.update()
    populate(engine, app.manager, 3, held=0, add=lambda kind: app.add_instrument(kind, defer=False))
    stream.start()
    app.root.update()
    base = stream.xruns
    times, end = [], time.time() + seconds
    while time.time() < end:
        t0 = time.perf_counter()
        app.add_instrument("synth")
        deadline = time.time() + 20
        while len(app.panels) < len(app.manager.instances) and time.time() < deadline:
            app.root.update()
            time.sleep(0.001)
        times.append((time.perf_counter() - t0) * 1000)
        for _ in range(30):
            app.root.update()
            time.sleep(0.01)
        if len(app.manager.instances) > 6:
            app.close_instance(app.manager.instances[-1])
    out = {"tabs_created": len(times), "panel_build_ms_avg": round(sum(times) / len(times), 1),
           "panel_build_ms_max": round(max(times), 1), "xruns_during": stream.xruns - base,
           "errors": stream.errors, "max_late_ms": round(stream.max_late_ms, 2)}
    stream.stop()
    stream.close()
    app.close()
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--seconds", type=float, default=10.0)
    ap.add_argument("--instances", type=int, default=5)
    ap.add_argument("--no-churn", action="store_true")
    ap.add_argument("--held", type=int, default=8, help="voices held on one Synth (0 = typical load, 8 = worst case)")
    ap.add_argument("--gui", action="store_true")
    a = ap.parse_args(argv)
    res = run_gui(a.seconds) if a.gui else run(a.seconds, a.instances, not a.no_churn, a.held)
    print(res)
    return 1 if res["errors"] or res.get("engine_errors") or res.get("xruns", res.get("xruns_during", 0)) > 1 else 0


if __name__ == "__main__":
    sys.exit(main())
