"""
Build Petri net from parsed MIR.
Control flow: entry -> bb0 -> ... -> exit.
Mutex: p_mutex_<key>_free (init=1), p_mutex_<key>_held (init=0).
Resource constraints attached to CFG transitions.
"""

from typing import Optional

from mir_model import (
    BasicBlock,
    MirFunction,
    TerminatorCall,
    TerminatorDrop,
    TerminatorGoto,
    TerminatorReturn,
    TerminatorSwitch,
)
from pn_model import Arc, PetriNet, Place, Transition


def build_petri_net(
    functions: list[MirFunction],
    entry_fn: str = "main",
    max_fns: Optional[int] = None,
) -> PetriNet:
    """Build Petri net from parsed MIR functions."""
    net = PetriNet()
    seen_places: set[str] = set()
    seen_transitions: set[str] = set()
    arc_id_counter = [0]

    def next_arc_id() -> str:
        arc_id_counter[0] += 1
        return f"arc_{arc_id_counter[0]}"

    def add_place(place: Place) -> None:
        if place.id not in seen_places:
            seen_places.add(place.id)
            net.places.append(place)
            if place.init_tokens > 0:
                net.initial_marking[place.id] = place.init_tokens

    def add_transition(t: Transition) -> None:
        if t.id not in seen_transitions:
            seen_transitions.add(t.id)
            net.transitions.append(t)

    def add_arc(source: str, target: str, weight: int = 1) -> None:
        aid = next_arc_id()
        net.arcs.append(Arc(id=aid, source=source, target=target, weight=weight))

    fns_to_process = functions
    if max_fns is not None:
        fns_to_process = functions[:max_fns]

    for fn in fns_to_process:
        _build_function_net(
            fn, entry_fn, net, add_place, add_transition, add_arc
        )

    return net


def _build_function_net(
    fn: MirFunction,
    entry_fn: str,
    net: PetriNet,
    add_place: callable,
    add_transition: callable,
    add_arc: callable,
) -> None:
    """Build control flow and mutex for one function."""
    f = fn.name
    is_entry = f == entry_fn

    # Places
    p_entry = Place(
        id=f"p_{f}_entry",
        name=f"{f}_entry",
        kind="cfg",
        init_tokens=1 if is_entry else 0,
    )
    add_place(p_entry)

    p_exit = Place(
        id=f"p_{f}_exit",
        name=f"{f}_exit",
        kind="cfg",
        init_tokens=0,
    )
    add_place(p_exit)

    bb_to_place: dict[int, str] = {}
    for bb in fn.basic_blocks:
        if bb.is_cleanup:
            continue
        pid = f"p_{f}_bb{bb.bb_id}"
        bb_to_place[bb.bb_id] = pid
        add_place(
            Place(id=pid, name=f"{f}_bb{bb.bb_id}", kind="cfg", init_tokens=0)
        )

    # Start transition: entry -> bb0
    if fn.basic_blocks:
        first_bb = next((b for b in fn.basic_blocks if not b.is_cleanup), None)
        if first_bb is not None:
            t_start = Transition(
                id=f"t_{f}_start",
                name=f"{f}_start",
                kind="cfg",
            )
            add_transition(t_start)
            add_arc(p_entry.id, t_start.id, 1)
            add_arc(t_start.id, bb_to_place[first_bb.bb_id], 1)

    # CFG edges and terminator handling
    for bb in fn.basic_blocks:
        if bb.is_cleanup:
            continue
        src_place = bb_to_place.get(bb.bb_id)
        if not src_place:
            continue

        term = bb.terminator
        if term is None:
            net.warnings.append({
                "function": f,
                "basic_block": f"bb{bb.bb_id}",
                "line": bb.line_start,
                "reason": "no terminator found",
                "callee": None,
            })
            continue

        targets: list[int] = []
        mutex_lock_key: str | None = None
        mutex_unlock_key: str | None = None

        if isinstance(term, TerminatorGoto):
            targets = [term.target_bb]
        elif isinstance(term, TerminatorReturn):
            targets = []  # goes to exit
        elif isinstance(term, TerminatorSwitch):
            targets = term.targets
        elif isinstance(term, TerminatorDrop):
            targets = [term.return_target]
            if term.local in fn.guard_to_mutex_key:
                mutex_unlock_key = fn.guard_to_mutex_key[term.local]
            else:
                net.warnings.append({
                    "function": f,
                    "basic_block": f"bb{bb.bb_id}",
                    "line": bb.line_start,
                    "reason": f"drop({term.local}) not in guard binding table",
                    "callee": "drop",
                })
        elif isinstance(term, TerminatorCall):
            targets = [term.return_target]
            callee = term.callee
            if ("Mutex" in callee and "lock" in callee) or "mutex::lock" in callee.lower():
                first_arg = _extract_first_local(term.args_str)
                if first_arg:
                    mutex_lock_key = _resolve_mutex_key(first_arg, fn.ref_to_base)
                else:
                    net.warnings.append({
                        "function": f,
                        "basic_block": f"bb{bb.bb_id}",
                        "line": bb.line_start,
                        "reason": "Mutex::lock call but no local in args",
                        "callee": callee,
                    })
            else:
                # Unrecognized call - treat as normal CFG, no crash
                net.warnings.append({
                    "function": f,
                    "basic_block": f"bb{bb.bb_id}",
                    "line": bb.line_start,
                    "reason": "unrecognized call, treated as CFG edge",
                    "callee": callee,
                })

        # Create transition for each target
        for target_bb in targets:
            t_id = f"t_{f}_bb{bb.bb_id}_to_bb{target_bb}"
            t = Transition(
                id=t_id,
                name=f"{f}_bb{bb.bb_id}_to_bb{target_bb}",
                kind="lock" if mutex_lock_key else ("unlock" if mutex_unlock_key else "cfg"),
                op=mutex_lock_key or mutex_unlock_key,
            )
            add_transition(t)
            add_arc(src_place, t.id, 1)
            dst_place = bb_to_place.get(target_bb)
            if dst_place:
                add_arc(t.id, dst_place, 1)

            # Mutex: attach resource constraints to this transition
            if mutex_lock_key:
                p_free = f"p_mutex_{mutex_lock_key}_free"
                p_held = f"p_mutex_{mutex_lock_key}_held"
                _ensure_mutex_places(net, add_place, mutex_lock_key)
                add_arc(p_free, t.id, 1)
                add_arc(t.id, p_held, 1)
            if mutex_unlock_key:
                p_free = f"p_mutex_{mutex_unlock_key}_free"
                p_held = f"p_mutex_{mutex_unlock_key}_held"
                _ensure_mutex_places(net, add_place, mutex_unlock_key)
                add_arc(p_held, t.id, 1)
                add_arc(t.id, p_free, 1)

        # Return: bb -> exit
        if isinstance(term, TerminatorReturn):
            t_return = Transition(
                id=f"t_{f}_bb{bb.bb_id}_return",
                name=f"{f}_bb{bb.bb_id}_return",
                kind="cfg",
            )
            add_transition(t_return)
            add_arc(src_place, t_return.id, 1)
            add_arc(t_return.id, p_exit.id, 1)


def _extract_first_local(args_str: str) -> str | None:
    import re
    m = re.search(r'(?:^|,)\s*(?:move\s+)?(_\d+)\b', args_str.strip())
    return m.group(1) if m else None


def _resolve_mutex_key(local: str, ref_to_base: dict[str, str]) -> str:
    return ref_to_base.get(local, local)


def _ensure_mutex_places(net: PetriNet, add_place: callable, key: str) -> None:
    """Ensure mutex free/held places exist."""
    p_free = Place(
        id=f"p_mutex_{key}_free",
        name=f"mutex_{key}_free",
        kind="mutex_free",
        init_tokens=1,
    )
    p_held = Place(
        id=f"p_mutex_{key}_held",
        name=f"mutex_{key}_held",
        kind="mutex_held",
        init_tokens=0,
    )
    add_place(p_free)
    add_place(p_held)
