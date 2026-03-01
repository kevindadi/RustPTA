"""
PNML export for mir2pnml_py.
PTNet format, pnml.org 2009 grammar.
Uses xml.etree.ElementTree (standard library).
"""

import xml.etree.ElementTree as ET

from pn_model import PetriNet


PNML_NS = "http://www.pnml.org/version-2009/grammar/ptnet"


def write_pnml(net: PetriNet, path: str) -> None:
    """Write Petri net to PNML file (PTNet 2009)."""
    root = ET.Element("pnml")
    net_elem = ET.SubElement(root, "net", id="mir2pnml_net", type=PNML_NS)

    # Page (required by some tools)
    page = ET.SubElement(net_elem, "page", id="page0")

    for p in net.places:
        place_elem = ET.SubElement(page, "place", id=p.id)
        name_elem = ET.SubElement(place_elem, "name")
        text_elem = ET.SubElement(name_elem, "text")
        text_elem.text = p.name
        if p.init_tokens > 0:
            init_elem = ET.SubElement(place_elem, "initialMarking")
            init_text = ET.SubElement(init_elem, "text")
            init_text.text = str(p.init_tokens)
        # Optional toolspecific
        if p.annotations:
            _add_toolspecific(place_elem, p.annotations)

    for t in net.transitions:
        trans_elem = ET.SubElement(page, "transition", id=t.id)
        name_elem = ET.SubElement(trans_elem, "name")
        text_elem = ET.SubElement(name_elem, "text")
        text_elem.text = t.name
        if t.annotations:
            _add_toolspecific(trans_elem, t.annotations)

    for a in net.arcs:
        arc_elem = ET.SubElement(page, "arc", id=a.id, source=a.source, target=a.target)
        if a.weight != 1:
            inscr = ET.SubElement(arc_elem, "inscription")
            inscr_text = ET.SubElement(inscr, "text")
            inscr_text.text = str(a.weight)

    tree = ET.ElementTree(root)
    _indent(root)
    tree.write(path, encoding="unicode", default_namespace="", method="xml")


def _add_toolspecific(elem: ET.Element, annotations: dict) -> None:
    """Add toolspecific element with annotations."""
    ts = ET.SubElement(elem, "toolspecific", tool="mir2pnml_py", version="0.1")
    for k, v in annotations.items():
        child = ET.SubElement(ts, k)
        child.text = str(v)


def _indent(elem: ET.Element, level: int = 0) -> None:
    """Pretty-print indentation."""
    i = "\n" + level * "  "
    if len(elem):
        if not elem.text or not elem.text.strip():
            elem.text = i + "  "
        if not elem.tail or not elem.tail.strip():
            elem.tail = i
        for child in elem:
            _indent(child, level + 1)
        if not child.tail or not child.tail.strip():
            child.tail = i
    else:
        if level and (not elem.tail or not elem.tail.strip()):
            elem.tail = i
