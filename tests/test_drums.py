import numpy as np
import pytest

from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS, STEPS, render_one_shot

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
