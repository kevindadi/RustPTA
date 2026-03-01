"""
MIR text parser for mir2pnml_py.
Regex-driven parser for rustc MIR dump format (e.g. rustc --emit=mir -Z unpretty=mir).
"""

import re
from typing import Optional

from mir_model import (
    BasicBlock,
    LocalDecl,
    MirFunction,
    TerminatorCall,
    TerminatorDrop,
    TerminatorGoto,
    TerminatorReturn,
    TerminatorSwitch,
)


class ParseError(Exception):
    """Raised when MIR parsing fails."""

    def __init__(self, message: str, function: str = "", basic_block: str = "", line: int = 0):
        self.function = function
        self.basic_block = basic_block
        self.line = line
        parts = []
        if function:
            parts.append(f"function {function}")
        if basic_block:
            parts.append(f"basic block {basic_block}")
        if line:
            parts.append(f"near line {line}")
        if parts:
            full_msg = f"{message} (in {' / '.join(parts)})"
        else:
            full_msg = message
        super().__init__(full_msg)


def _extract_first_local(args_str: str) -> Optional[str]:
    """Extract first local _N from args string (e.g. 'move _4' or '_1, const 0')."""
    m = re.search(r'(?:^|,)\s*(?:move\s+)?(_\d+)\b', args_str.strip())
    return m.group(1) if m else None


def _resolve_mutex_key(local: str, ref_to_base: dict[str, str]) -> str:
    """Resolve ref local to base mutex. E.g. _4 = &_1 -> _1."""
    return ref_to_base.get(local, local)


def parse_mir(text: str) -> list[MirFunction]:
    """
    Parse MIR text and return list of MirFunction.
    Performs guard-to-mutex binding and unwrap/expect propagation.
    """
    functions: list[MirFunction] = []
    fn_pattern = re.compile(r"fn\s+(\w+)\s*\([^)]*\)\s*->\s*[^{]*\{")
    pos = 0
    while True:
        m = fn_pattern.search(text, pos)
        if not m:
            break
        fn_name = m.group(1)
        fn_start = text[: m.start()].count("\n") + 1
        # Find matching brace
        start = m.end() - 1  # position of {
        depth = 1
        i = m.end()
        while i < len(text) and depth > 0:
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
            i += 1
        fn_body = text[m.end() : i - 1]
        try:
            func = _parse_function_body(fn_name, fn_body, fn_start)
            functions.append(func)
        except ParseError:
            raise
        except Exception as e:
            raise ParseError(str(e), function=fn_name, line=fn_start) from e
        pos = i
    return functions


def _parse_function_body(fn_name: str, body: str, fn_start_line: int) -> MirFunction:
    """Parse function body: locals, ref_to_base, basic blocks."""
    let_pattern = re.compile(r"let\s+(mut\s+)?(_\d+)\s*:\s*([^;]+);")
    ref_pattern = re.compile(r"(_\d+)\s*=\s*&(_\d+)\s*;")
    bb_pattern = re.compile(r"bb(\d+)\s*(?:\(cleanup\))?\s*:\s*\{")
    goto_pattern = re.compile(r"goto\s*->\s*bb(\d+)\s*;")
    return_pattern = re.compile(r"return\s*;")
    drop_pattern = re.compile(
        r"drop\s*\(([^)]+)\)\s*->\s*\[return:\s*bb(\d+)(?:,\s*unwind:\s*(?:bb(\d+)|continue|terminate[^]]*))?\]\s*;"
    )
    switch_pattern = re.compile(r"switchInt\s*\([^)]+\)\s*->\s*\[([^\]]+)\]\s*;")
    call_pattern = re.compile(
        r"(?:(\w+)\s*=\s*)?([^(]+)\(([^)]*)\)\s*->\s*\[return:\s*bb(\d+)(?:,\s*unwind:\s*(?:bb(\d+)|continue|terminate[^]]*))?\]\s*;"
    )

    locals_list: list[LocalDecl] = []
    ref_to_base: dict[str, str] = {}
    basic_blocks: list[BasicBlock] = []
    guard_to_mutex_key: dict[str, str] = {}

    lines = body.split("\n")
    seen_bb = False
    i = 0

    while i < len(lines):
        ln = lines[i]
        stripped = ln.strip()

        # Skip scope/debug
        if re.match(r"scope\s+\d+", stripped) or re.match(r"debug\s+", stripped):
            i += 1
            continue

        # Locals only before first bb
        let_m = let_pattern.search(stripped)
        if let_m and not seen_bb:
            is_mut = let_m.group(1) is not None
            locals_list.append(
                LocalDecl(
                    name=let_m.group(2),
                    ty_str=let_m.group(3).strip(),
                    is_mut=is_mut,
                )
            )
            i += 1
            continue

        # Basic block
        bb_m = bb_pattern.match(stripped)
        if bb_m:
            seen_bb = True
            bb_id = int(bb_m.group(1))
            is_cleanup = "(cleanup)" in ln
            line_start = fn_start_line + i + 1
            block_lines: list[str] = []
            j = i + 1
            while j < len(lines):
                bl = lines[j]
                bl_stripped = bl.strip()
                if bb_pattern.match(bl_stripped):
                    break
                if bl_stripped == "}":
                    j += 1
                    break
                block_lines.append(bl)
                # Ref assignment (for mutex key resolution)
                ref_m = ref_pattern.search(bl_stripped)
                if ref_m:
                    ref_to_base[ref_m.group(1)] = ref_m.group(2)
                j += 1

            # Find terminator (last line that matches a terminator pattern)
            terminator = None
            for bl in reversed(block_lines):
                bs = bl.strip()
                if goto_pattern.search(bs):
                    tm = goto_pattern.search(bs)
                    if tm:
                        terminator = TerminatorGoto(target_bb=int(tm.group(1)))
                        break
                if return_pattern.search(bs):
                    terminator = TerminatorReturn()
                    break
                if drop_pattern.search(bs):
                    dm = drop_pattern.search(bs)
                    if dm:
                        terminator = TerminatorDrop(
                            local=dm.group(1).strip(),
                            return_target=int(dm.group(2)),
                            unwind_target=int(dm.group(3)) if dm.group(3) else None,
                        )
                        break
                if switch_pattern.search(bs):
                    sm = switch_pattern.search(bs)
                    if sm:
                        targets = [int(x) for x in re.findall(r"bb(\d+)", sm.group(1))]
                        terminator = TerminatorSwitch(targets=targets)
                        break
                if call_pattern.search(bs):
                    cm = call_pattern.search(bs)
                    if cm:
                        lhs = cm.group(1)
                        callee = cm.group(2).strip()
                        args_str = cm.group(3).strip()
                        terminator = TerminatorCall(
                            lhs=lhs,
                            callee=callee,
                            args_str=args_str,
                            return_target=int(cm.group(4)),
                            unwind_target=int(cm.group(5)) if cm.group(5) else None,
                        )
                        # Guard binding: Mutex::lock (handles Mutex::<T>::lock)
                        if ("Mutex" in callee and "lock" in callee) or "mutex::lock" in callee.lower():
                            first_arg = _extract_first_local(args_str)
                            if first_arg:
                                mutex_key = _resolve_mutex_key(first_arg, ref_to_base)
                                if lhs:
                                    guard_to_mutex_key[lhs] = mutex_key
                        # unwrap/expect propagation
                        elif ("::unwrap" in callee or "::expect" in callee) and lhs:
                            first_arg = _extract_first_local(args_str)
                            if first_arg and first_arg in guard_to_mutex_key:
                                guard_to_mutex_key[lhs] = guard_to_mutex_key[first_arg]
                        break

            basic_blocks.append(
                BasicBlock(
                    bb_id=bb_id,
                    statements=block_lines[:-1] if terminator and block_lines else block_lines,
                    terminator=terminator,
                    is_cleanup=is_cleanup,
                    line_start=line_start,
                )
            )
            i = j
            continue

        i += 1

    return MirFunction(
        name=fn_name,
        locals=locals_list,
        basic_blocks=basic_blocks,
        guard_to_mutex_key=guard_to_mutex_key,
        ref_to_base=ref_to_base,
    )
