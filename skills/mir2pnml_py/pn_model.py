"""
Petri net data structures for mir2pnml_py.
Bipartite graph: Place <-> Transition only.
"""

from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class Place:
    """Place in the Petri net."""
    id: str
    name: str
    kind: str = "cfg"  # cfg, mutex_free, mutex_held
    init_tokens: int = 0
    capacity: Optional[int] = None
    annotations: dict[str, Any] = field(default_factory=dict)


@dataclass
class Transition:
    """Transition in the Petri net."""
    id: str
    name: str
    kind: str = "cfg"  # cfg, lock, unlock
    op: Optional[str] = None
    annotations: dict[str, Any] = field(default_factory=dict)


@dataclass
class Arc:
    """Arc between place and transition (bipartite: place->transition or transition->place)."""
    id: str
    source: str
    target: str
    weight: int = 1


@dataclass
class PetriNet:
    """Petri net with places, transitions, arcs, and optional warnings."""
    places: list[Place] = field(default_factory=list)
    transitions: list[Transition] = field(default_factory=list)
    arcs: list[Arc] = field(default_factory=list)
    initial_marking: dict[str, int] = field(default_factory=dict)
    warnings: list[dict[str, Any]] = field(default_factory=list)

    def place_by_id(self, pid: str) -> Optional[Place]:
        for p in self.places:
            if p.id == pid:
                return p
        return None

    def transition_by_id(self, tid: str) -> Optional[Transition]:
        for t in self.transitions:
            if t.id == tid:
                return t
        return None
