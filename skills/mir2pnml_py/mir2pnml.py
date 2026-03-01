#!/usr/bin/env python3
"""
mir2pnml_py: MIR text -> PNML Petri net (PTNet).
CLI entry point. Uses only Python standard library.
"""

import argparse
import json
import sys
from pathlib import Path

# Add parent of script dir for imports when run as script
_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from mir_parser import ParseError, parse_mir
from pn_builder import build_petri_net
from pn_model import PetriNet
from pnml_writer import write_pnml


def _net_to_json_serializable(net: PetriNet) -> dict:
    """Convert PetriNet to JSON-serializable dict."""
    return {
        "places": [
            {
                "id": p.id,
                "name": p.name,
                "kind": p.kind,
                "init_tokens": p.init_tokens,
            }
            for p in net.places
        ],
        "transitions": [
            {"id": t.id, "name": t.name, "kind": t.kind, "op": t.op}
            for t in net.transitions
        ],
        "arcs": [
            {"id": a.id, "source": a.source, "target": a.target, "weight": a.weight}
            for a in net.arcs
        ],
        "initial_marking": net.initial_marking,
        "warnings": net.warnings,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Convert rustc MIR text to PNML Petri net (PTNet)."
    )
    parser.add_argument("--mir", required=True, help="Input MIR text file")
    parser.add_argument("--out", required=True, help="Output PNML file path")
    parser.add_argument(
        "--dump-json",
        metavar="FILE",
        help="Optional: dump internal Petri net as JSON for debugging",
    )
    parser.add_argument(
        "--entry-fn",
        default="main",
        help="Entry function name (default: main)",
    )
    parser.add_argument(
        "--rwlock-n",
        type=int,
        default=8,
        metavar="N",
        help="RwLock read concurrency token limit (default: 8, reserved for extension)",
    )
    parser.add_argument(
        "--max-fns",
        type=int,
        default=None,
        metavar="N",
        help="Max number of functions to parse (default: no limit)",
    )

    args = parser.parse_args()
    # rwlock_n is reserved for future use
    _ = args.rwlock_n

    mir_path = Path(args.mir)
    if not mir_path.exists():
        print(f"Error: MIR file not found: {mir_path}", file=sys.stderr)
        return 1

    try:
        text = mir_path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        print(f"Error: cannot read MIR file: {e}", file=sys.stderr)
        return 1

    try:
        functions = parse_mir(text)
    except ParseError as e:
        print(f"Parse error: {e}", file=sys.stderr)
        return 1

    if not functions:
        print("Error: no functions parsed from MIR", file=sys.stderr)
        return 1

    net = build_petri_net(
        functions,
        entry_fn=args.entry_fn,
        max_fns=args.max_fns,
    )

    try:
        write_pnml(net, args.out)
    except OSError as e:
        print(f"Error: cannot write PNML: {e}", file=sys.stderr)
        return 1

    if args.dump_json:
        dump_data = _net_to_json_serializable(net)
        try:
            Path(args.dump_json).write_text(
                json.dumps(dump_data, indent=2, ensure_ascii=False),
                encoding="utf-8",
            )
        except OSError as e:
            print(f"Warning: cannot write dump-json: {e}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
