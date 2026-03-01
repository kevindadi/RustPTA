"""
MIR data structures for mir2pnml_py.
Uses dataclasses for representation of parsed MIR text.
"""

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class LocalDecl:
    """Local variable declaration: let [mut] _N: Type;"""
    name: str
    ty_str: str
    is_mut: bool = False


@dataclass
class TerminatorGoto:
    """goto -> bbN;"""
    target_bb: int


@dataclass
class TerminatorReturn:
    """return;"""
    pass


@dataclass
class TerminatorSwitch:
    """switchInt(...) -> [targets...];"""
    targets: list[int]


@dataclass
class TerminatorDrop:
    """drop(local) -> [return: bbN, unwind: ...];"""
    local: str
    return_target: int
    unwind_target: Optional[int] = None


@dataclass
class TerminatorCall:
    """lhs = callee(args) -> [return: bbN, unwind: ...]; or callee(args) -> ..."""
    lhs: Optional[str]
    callee: str
    args_str: str
    return_target: int
    unwind_target: Optional[int] = None


# Union type for terminator
Terminator = TerminatorGoto | TerminatorReturn | TerminatorSwitch | TerminatorDrop | TerminatorCall


@dataclass
class BasicBlock:
    """Basic block with optional statements and terminator."""
    bb_id: int
    statements: list[str] = field(default_factory=list)
    terminator: Optional[Terminator] = None
    is_cleanup: bool = False
    line_start: int = 0  # approximate line for error reporting


@dataclass
class MirFunction:
    """Parsed MIR function with locals, basic blocks, and guard bindings."""
    name: str
    locals: list[LocalDecl] = field(default_factory=list)
    basic_blocks: list[BasicBlock] = field(default_factory=list)
    guard_to_mutex_key: dict[str, str] = field(default_factory=dict)
    # ref_to_base: _4 -> _1 when _4 = &_1 (for resolving mutex key from lock args)
    ref_to_base: dict[str, str] = field(default_factory=dict)
