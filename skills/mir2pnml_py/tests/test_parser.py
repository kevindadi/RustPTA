"""
Unit tests for MIR parser.
Uses unittest (standard library only).
"""

import sys
import unittest
from pathlib import Path

# Add parent of tests dir for imports
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from mir_model import TerminatorCall, TerminatorDrop, TerminatorReturn
from mir_parser import parse_mir


# Minimal MIR with lock and drop (simplified: lock takes _1 directly)
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


class TestParser(unittest.TestCase):
    def test_parse_function_name(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        self.assertEqual(len(funcs), 1)
        self.assertEqual(funcs[0].name, "main")

    def test_parse_basic_blocks(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        bb_ids = [bb.bb_id for bb in funcs[0].basic_blocks]
        self.assertIn(0, bb_ids)
        self.assertIn(1, bb_ids)
        self.assertIn(2, bb_ids)

    def test_parse_cfg_edges(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        fn = funcs[0]
        # bb0 -> bb1 (lock return)
        bb0 = next(b for b in fn.basic_blocks if b.bb_id == 0)
        self.assertIsNotNone(bb0.terminator)
        if isinstance(bb0.terminator, TerminatorCall):
            self.assertEqual(bb0.terminator.return_target, 1)
        # bb1 -> bb2 (drop return)
        bb1 = next(b for b in fn.basic_blocks if b.bb_id == 1)
        if isinstance(bb1.terminator, TerminatorDrop):
            self.assertEqual(bb1.terminator.return_target, 2)
        # bb2 -> return
        bb2 = next(b for b in fn.basic_blocks if b.bb_id == 2)
        self.assertIsInstance(bb2.terminator, TerminatorReturn)

    def test_parse_call_terminator(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        bb0 = next(b for b in funcs[0].basic_blocks if b.bb_id == 0)
        self.assertIsInstance(bb0.terminator, TerminatorCall)
        call = bb0.terminator
        self.assertEqual(call.lhs, "_2")
        self.assertIn("Mutex", call.callee)
        self.assertIn("lock", call.callee)
        self.assertIn("_1", call.args_str)

    def test_parse_drop_terminator(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        bb1 = next(b for b in funcs[0].basic_blocks if b.bb_id == 1)
        self.assertIsInstance(bb1.terminator, TerminatorDrop)
        drop = bb1.terminator
        self.assertEqual(drop.local, "_2")

    def test_guard_binding(self) -> None:
        funcs = parse_mir(MINIMAL_MIR)
        fn = funcs[0]
        # _2 is guard from lock(move _1), so guard_to_mutex_key[_2] = _1
        self.assertIn("_2", fn.guard_to_mutex_key)
        self.assertEqual(fn.guard_to_mutex_key["_2"], "_1")


if __name__ == "__main__":
    unittest.main()
