import importlib.util
from pathlib import Path

import pytest

from nomad.audio.stream import list_output_devices

pytestmark = pytest.mark.skipif(not list_output_devices(), reason="no audio output device")


def _tool():
    spec = importlib.util.spec_from_file_location("stream_stress", Path(__file__).parent.parent / "tools" / "stream_stress.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_instance_churn_on_real_stream():
    res = _tool().run(seconds=5.0, instances=5, with_churn=True, held=0)
    print(res)
    assert res["errors"] == 0 and res["engine_errors"] == 0 and res["xruns"] <= 1
    assert res["callbacks"] > 50 and res["churn_ops"] > 20


def test_five_instruments_sound_together_without_xruns():
    # typical load (sequences + drums). 8 held voices on top of it is NOT covered: it overruns the CPU budget
    # (see README, 'Límites conocidos'); tools/stream_stress.py --held 8 reproduces it.
    res = _tool().run(seconds=4.0, instances=5, with_churn=False, held=0)
    print(res)
    assert res["instances"] == 5 and res["errors"] == 0 and res["xruns"] <= 1
