---
name: mir2pnml-py
description: Parses rustc MIR text into PNML Petri net (PTNet) for control flow and Mutex mutual exclusion modeling. Use when converting MIR dumps to Petri nets, comparing with RustPTA, or extending mutex/rwlock modeling.
---

# MIR Text -> PNML Petri Net (Python)

## Input / Output

| Input | Output |
|-------|--------|
| MIR text file (from `rustc --emit=mir -Z unpretty=mir`) | PNML (PTNet 2009), optional JSON dump |

## CLI

```bash
python skills/mir2pnml_py/mir2pnml.py --mir <MIR_FILE> --out <PNML_FILE> [--dump-json <JSON_FILE>] [--entry-fn main] [--rwlock-n 8] [--max-fns N]
```

## Algorithm

1. **Parse** — Regex-driven parse of MIR: functions, basic blocks, terminators (goto, return, switchInt, drop, call)
2. **CFG** — Extract control-flow edges from terminators
3. **Bind guards** — Lock call lhs -> mutex key (from args first local); unwrap/expect propagate binding
4. **Build net** — Control-flow places/transitions; mutex places `p_mutex_<key>_free` (init=1), `p_mutex_<key>_held` (init=0); attach resource arcs to CFG transitions
5. **Write PNML** — PTNet XML via `xml.etree.ElementTree`

## Failure strategy

- Parse error: raise with `function / basic block / near line / reason`
- Unrecognized terminator/call: do not crash; treat as normal CFG edge; record warning in `dump-json` (and optional PNML toolspecific)

## Extension points

- **RwLock** — Use `--rwlock-n`; add read/write place pairs
- **spawn/join** — Thread creation/join; approximate with extra places/transitions
- **Atomic** — Load/store as self-loop on resource place
- **Alias hints** — External alias analysis can feed `ref_to_base` mapping
