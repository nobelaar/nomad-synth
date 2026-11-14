import pytest

from nomad.engine.modmatrix import DESTINATIONS, SOURCES, ModMatrix


def test_sums_two_sources_on_same_destination():
    m = ModMatrix()
    m.set_slot(0, 1, 5, 0.5)
    m.set_slot(1, 2, 5, 0.25)
    assert m.resolve({"lfo1": 1.0, "lfo2": 1.0})["cutoff"] == 0.75


def test_off_slots_contribute_nothing():
    m = ModMatrix()
    m.set_slot(0, 0, 0, 1.0)
    m.set_slot(1, 0, 1, 1.0)  # src off
    m.set_slot(2, 1, 0, 1.0)  # dst off
    assert all(v == 0.0 for v in m.resolve({"lfo1": 1.0}).values())


def test_amount_is_bipolar_and_clamped():
    m = ModMatrix()
    m.set_slot(0, 1, 1, -2.0)
    assert m.resolve({"lfo1": 1.0})["pitch"] == -1.0


def test_destination_tables_are_stable():
    assert SOURCES[0] == "off" and DESTINATIONS[0] == "off"
    assert len(DESTINATIONS) == 8


@pytest.mark.parametrize("args", [(-1, 0, 0), (8, 0, 0), (0, -1, 0), (0, 6, 0), (0, 0, -1), (0, 0, 8)])
def test_invalid_indices_raise(args):
    with pytest.raises(ValueError):
        ModMatrix().set_slot(*args, 0.5)


@pytest.mark.parametrize("amt", [float("nan"), float("inf"), float("-inf")])
def test_non_finite_amount_is_zero(amt):
    m = ModMatrix()
    m.set_slot(0, 1, 1, amt)
    assert m.resolve({"lfo1": 1.0})["pitch"] == 0.0


def test_missing_sources_count_as_zero():
    m = ModMatrix()
    m.set_slot(0, 5, 4, 1.0)
    assert m.resolve({})["amp"] == 0.0


def test_repeated_resolve_does_not_accumulate():
    m = ModMatrix()
    m.set_slot(0, 1, 1, 0.5)
    assert m.resolve({"lfo1": 1.0})["pitch"] == 0.5
    assert m.resolve({"lfo1": 1.0})["pitch"] == 0.5
    assert m.resolve({"lfo1": 0.0})["pitch"] == 0.0


def test_all_eight_slots_at_once():
    m = ModMatrix()
    for i in range(8):
        m.set_slot(i, 1, 1 + i % 7, 0.5)
    out = m.resolve({"lfo1": 1.0})
    assert out["pitch"] == 1.0  # slots 0 and 7 both hit pitch
    assert all(out[d] == 0.5 for d in DESTINATIONS[2:])
