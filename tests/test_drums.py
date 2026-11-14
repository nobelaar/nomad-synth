import numpy as np
import pytest

from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS, STEPS, DrumMachine, render_one_shot

SR = 48000.0


@pytest.mark.parametrize("kind", INSTRUMENTS)
def test_one_shot_is_a_decaying_sound(kind):
    y = render_one_shot(kind, SR)
    assert y.dtype == np.float32 and y.ndim == 1
    assert 0.03 * SR < len(y) <= 1.5 * SR
    assert np.abs(y).max() > 0.1
    assert np.abs(y[-200:]).max() < 0.02 * np.abs(y).max()


def test_one_shot_is_deterministic():
    assert np.array_equal(render_one_shot("snare", SR), render_one_shot("snare", SR))


def test_tune_raises_kick_pitch_and_decay_lengthens_it():
    base = render_one_shot("kick", SR, tune=0.0, decay=0.5)
    higher = render_one_shot("kick", SR, tune=0.8, decay=0.5)
    longer = render_one_shot("kick", SR, tune=0.0, decay=0.9)
    peak = lambda y: int(np.argmax(np.abs(np.fft.rfft(y))))  # noqa: E731
    assert peak(higher) > peak(base)
    loud = lambda y: np.count_nonzero(np.abs(y) > 0.01 * np.abs(y).max())  # noqa: E731
    assert loud(longer) > loud(base)


def test_bad_arguments_are_clamped_or_rejected():
    render_one_shot("kick", SR, tune=99.0, decay=float("nan"))
    with pytest.raises(ValueError):
        render_one_shot("cowbell", SR)


def test_registry_has_the_twenty_drum_params():
    expected = {f"{t}_{s}" for t in INSTRUMENTS for s in ("tune", "decay", "level")}
    assert set(DRUM_REGISTRY) == expected | {"bpm", "swing"}
    assert DRUM_REGISTRY["bpm"].lo == 60 and DRUM_REGISTRY["bpm"].hi == 180
    assert DRUM_REGISTRY["swing"].hi == pytest.approx(0.6)
    assert DRUM_REGISTRY["kick_level"].default == pytest.approx(0.6)
    assert DRUM_REGISTRY["ohat_level"].default == pytest.approx(0.3)
    assert STEPS == 16


def make_dm(bpm=120.0):
    dm = DrumMachine(SR)
    dm.clear_pattern()
    dm.set_value("bpm", bpm)
    dm.set_step("chat", 0, 1)
    return dm


def render(dm, sizes):
    chunks = []
    for n in sizes:
        b = np.zeros(n)
        dm.render(b, n)
        chunks.append(b)
    return np.concatenate(chunks)


def test_default_pattern_makes_sound_when_playing():
    dm = DrumMachine(SR)
    assert dm.steps[0, 0] == 2 and dm.steps[0, 4] == 1 and dm.steps[1, 4] == 1
    dm.start()
    out = render(dm, [256] * 8)
    assert np.abs(out).max() > 0.01


def test_position_advances_exactly_at_the_step_boundary():
    dm = make_dm(120.0)          # 16th a 120 BPM = 6000 frames
    dm.start()
    render(dm, [5999])
    assert dm.position == 0
    render(dm, [1])
    assert dm.position == 1


def test_block_size_does_not_change_the_groove():
    a = render(make_dm(), [16384])
    b = render(make_dm(), [256] * 64)
    c = render(make_dm(), [37, 1, 500, 128, 1000, 3000, 2000, 9718])
    assert np.allclose(a, b, atol=1e-9)
    assert np.allclose(a, c, atol=1e-9)


def test_accent_is_louder_than_a_normal_step():
    quiet, loud = make_dm(), make_dm()
    quiet.set_step("chat", 0, 1)
    loud.set_step("chat", 0, 2)
    for dm in (quiet, loud):
        dm.start()
    assert np.abs(render(loud, [4000])).max() > 1.4 * np.abs(render(quiet, [4000])).max()


def test_retrigger_restarts_the_hit():
    dm = make_dm()
    dm.trigger("kick")
    render(dm, [15000])          # deja decaer el golpe anterior
    tail = np.abs(render(dm, [4000])).max()
    dm.trigger("kick")
    assert np.abs(render(dm, [4000])).max() > 2 * tail


def test_tone_change_while_the_hit_is_playing_is_safe():
    dm = DrumMachine(SR)
    dm.trigger("kick")
    render(dm, [256])
    dm.set_value("kick_decay", 0.1)
    dm.set_value("kick_decay", 0.9)   # array nuevo, offset viejo más grande
    out = render(dm, [4096])
    assert np.isfinite(out).all()


def test_start_is_not_retriggered_and_stop_returns_to_step_zero():
    dm = make_dm()
    dm.start()
    dm.start()
    render(dm, [6000])
    assert dm.position == 1
    dm.stop()
    assert dm.position == 0 and not dm.playing
    dm.start()
    assert dm.position == 0


def test_panic_cuts_everything_within_five_ms():
    dm = DrumMachine(SR)
    dm.set_step("ohat", 0, 2)
    dm.start()
    render(dm, [480])
    dm.stop_all()
    tail = render(dm, [480])
    assert not dm.playing and dm.position == 0
    assert np.abs(tail[240:]).max() < 1e-3


def test_bpm_and_swing_apply_without_retriggering():
    dm = make_dm(120.0)
    dm.start()
    render(dm, [3000])
    dm.set_value("bpm", 180.0)
    render(dm, [3000])            # el paso 0 no se repite
    assert dm.position == 1
    dm.set_value("swing", 0.6)
    render(dm, [2000])
    assert dm.position == 2       # 180 BPM = 4000 frames por paso, swing par = 6400


def test_values_are_clamped_and_unknown_names_raise():
    dm = DrumMachine(SR)
    dm.set_value("bpm", 9999.0)
    dm.set_value("swing", float("nan"))
    assert dm.values["bpm"] == 180.0
    assert dm.values["swing"] == 0.0
    with pytest.raises(KeyError):
        dm.set_value("cowbell_level", 1.0)
    with pytest.raises(IndexError):
        dm.set_step("kick", 16, 1)


def test_render_does_not_allocate(monkeypatch):
    dm = DrumMachine(SR)
    dm.start()
    out = np.zeros(256)
    dm.render(out, 256)
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1
            return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(40):
        dm.render(out, 256)
    assert calls["n"] == 0


def render_engine(engine, blocks):
    chunks = []
    for _ in range(blocks):
        b = np.zeros(256)
        engine.render(b)
        chunks.append(b)
    return np.concatenate(chunks)


def test_drums_stopped_leaves_engine_output_identical():
    from nomad.engine.engine import Engine
    a, b = Engine(SR), Engine(SR)
    for e in (a, b):
        e.note_on(60, 0.8)
    assert np.array_equal(render_engine(a, 8), render_engine(b, 8))


def test_drums_are_audible_without_any_note():
    from nomad.engine.engine import Engine
    e = Engine(SR)
    e.drums.start()
    out = render_engine(e, 16)
    assert np.abs(out).max() > 0.01


def test_panic_silences_the_drums():
    from nomad.engine.engine import Engine
    e = Engine(SR)
    e.drums.start()
    render_engine(e, 4)
    e.panic()
    tail = render_engine(e, 4)
    assert np.abs(tail[240:]).max() < 1e-3
    assert not e.drums.playing


def test_rapid_tone_changes_regenerate_at_most_once(monkeypatch):
    import nomad.engine.drums as mod
    calls = []
    real = mod.render_one_shot

    def counting(*a, **k):
        calls.append(a[0])
        return real(*a, **k)

    monkeypatch.setattr(mod, "render_one_shot", counting)
    dm = DrumMachine(SR)
    calls.clear()
    dm.set_value("kick_decay", 0.9)
    dm.set_value("kick_decay", 0.8)
    dm.set_value("kick_decay", 0.7)
    assert len(calls) <= 1
    dm.flush()
    dm.flush()
    assert len(calls) <= 2
    assert dm.values["kick_decay"] == pytest.approx(0.7)


def test_bpm_jump_mid_step_does_not_skip_steps():
    dm = make_dm(60.0)               # paso de 12000 frames
    dm.start()
    render(dm, [11040])              # 92 % del paso 0
    assert dm.position == 0
    dm.set_value("bpm", 180.0)       # paso de 4000 frames
    render(dm, [384])                # 320 frames eran el resto del paso
    assert dm.position == 1


def test_swing_jump_mid_step_does_not_skip_steps():
    dm = make_dm(120.0)              # paso par de 6000 frames
    dm.start()
    render(dm, [5000])
    dm.set_value("swing", 0.6)       # el paso par pasa a 9600 frames
    render(dm, [1800])
    assert dm.position == 1


def test_tune_changes_the_noise_instruments():
    def centroid(y):
        spec = np.abs(np.fft.rfft(y))
        return float((spec * np.arange(len(spec))).sum() / spec.sum())

    for kind in ("chat", "ohat", "clap"):
        lo = render_one_shot(kind, SR, tune=-1.0)
        hi = render_one_shot(kind, SR, tune=1.0)
        assert centroid(hi) > centroid(lo) * 1.1, kind
