"""
End-to-end tests: MIR -> Petri net -> PNML.
Uses unittest (standard library only).
"""

import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from mir_parser import parse_mir
from pn_builder import build_petri_net
from pnml_writer import write_pnml

# Same minimal MIR as test_parser
MINIMAL_MIR = """
fn main() -> () {
    let _1: std::sync::Mutex<i32>;
    let _2: std::sync::MutexGuard<'_, i32>;
    bb0: {
        _2 = std::sync::Mutex::<i32>::lock(move _1) -> [return: bb1, unwind: bb2];
    }
    bb1: {
        drop(_2) -> [return: bb2, unwind: bb2];
    }
    bb2: {
        return;
    }
}
"""


class TestEndToEnd(unittest.TestCase):
    def test_mutex_places_exist(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        net = build_petri_net(funcs, entry_fn="main")
        places_by_id = {p.id: p for p in net.places}
        self.assertIn("p_mutex__1_free", places_by_id)
        self.assertIn("p_mutex__1_held", places_by_id)
        p_free = places_by_id["p_mutex__1_free"]
        p_held = places_by_id["p_mutex__1_held"]
        self.assertEqual(p_free.init_tokens, 1)
        self.assertEqual(p_held.init_tokens, 0)

    def test_lock_transition_has_mutex_arcs(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        net = build_petri_net(funcs, entry_fn="main")
        arcs_src_tgt = [(a.source, a.target) for a in net.arcs]
        # Lock transition: input from p_mutex__1_free, output to p_mutex__1_held
        lock_trans = next(
            (t for t in net.transitions if t.kind == "lock" and t.op == "_1"),
            None,
        )
        self.assertIsNotNone(lock_trans)
        tid = lock_trans.id
        self.assertIn(("p_mutex__1_free", tid), arcs_src_tgt)
        self.assertIn((tid, "p_mutex__1_held"), arcs_src_tgt)

    def test_drop_transition_has_mutex_arcs(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        net = build_petri_net(funcs, entry_fn="main")
        unlock_trans = next(
            (t for t in net.transitions if t.kind == "unlock" and t.op == "_1"),
            None,
        )
        self.assertIsNotNone(unlock_trans)
        tid = unlock_trans.id
        arcs_src_tgt = [(a.source, a.target) for a in net.arcs]
        self.assertIn(("p_mutex__1_held", tid), arcs_src_tgt)
        self.assertIn((tid, "p_mutex__1_free"), arcs_src_tgt)

    def test_pnml_output_valid(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        net = build_petri_net(funcs, entry_fn="main")
        with tempfile.NamedTemporaryFile(suffix=".pnml", delete=False) as f:
            path = f.name
        try:
            write_pnml(net, path)
            tree = ET.parse(path)
            root = tree.getroot()
            places = root.findall(".//{http://www.pnml.org/version-2009/grammar/ptnet}place")
            # PNML may not use namespace in our output - try without
            if not places:
                places = root.findall(".//place")
            transitions = root.findall(".//transition")
            arcs = root.findall(".//arc")
            self.assertGreater(len(places), 0)
            self.assertGreater(len(transitions), 0)
            self.assertGreater(len(arcs), 0)
        finally:
            Path(path).unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
