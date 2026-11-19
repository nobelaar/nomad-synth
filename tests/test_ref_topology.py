from pathlib import Path

from tools import ref_topology
from tools.refio import regenerate_matches

REF = Path(__file__).resolve().parent.parent / "rust" / "reference"


def test_reference_is_up_to_date(tmp_path):
    assert regenerate_matches(ref_topology.write, REF, tmp_path) == []


def test_rules_and_names():
    *_, rets, names = ref_topology.run(ref_topology.BLOBS["topology_c"]["topo/rules"])
    assert rets == [True, False, False, False, True, False, False, False, False, False]
    *_, names = ref_topology.run(ref_topology.BLOBS["topology_c"]["topo/add_names_ids"])
    assert names[-1] == [[1, "Synth 01"], [2, "Machine Drum 01"], [4, "Machine Drum 02"], [5, "Synth 03"],
                         [6, "Synth 04"]]
