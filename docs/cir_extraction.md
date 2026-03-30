# CIR Extraction from RustPTA

## Overview

**CIR** (Concurrency Intermediate Representation) is a YAML artifact describing synchronization structure and call relationships. It is produced **in parallel** with the Petri net: both use the same MIR translation and pointer-analysis resource ids, but CIR is **not** a mere re-export of `Net`.

## Pipeline

1. **Resource naming** — Scan Petri-net `TransitionType` values (same mutex / condvar / atomic / unsafe-var ids as the net). See [`resource_table.rs`](../src/cir/resource_table.rs).
2. **Net projection** — [`CirExtractor::extract`](../src/cir/net_extract.rs) walks per-function control-flow places (excluding cross-thread interleaving edges) and maps transitions to CIR ops.
3. **MIR calls + stubs** — [`merge_calls_and_stubs`](../src/cir/pipeline.rs) walks each in-scope function’s MIR (`TerminatorKind::Call`) and inserts [`call`](../src/cir/types.rs) operations, interleaved by basic block with net-derived ops. Functions with no sync transitions still appear as **stubs** (minimal `ret`) so the **call graph** stays complete.
4. **Scope** — [`def_in_scope`](../src/cir/pipeline.rs) mirrors `PetriNet::crate_filter_match` (crate name / white / black lists) and optionally restricts to the **`-f` input file** path when set.
5. **Protection** — [`infer_protection`](../src/cir/protection.rs) derives `protection: var -> [locks]` from lock stack order.

## CLI

With `--viz-cir`, analysis writes `cir.yaml` under the diagnostics directory (same tree as other `viz-*` outputs).

## What Gets Skipped as CIR Ops

| `TransitionType` | Reason |
| ------------------ | ------ |
| `Goto`, `Normal`, `Assert`, `Function`, `Inhibitor`, `Reset` | Internal / structural |
| `Start(_)` | Entry marker |
| `AsyncPoll`, `AwaitReady`, `AsyncAbort` | Internal async stepping (v1) |
| `Switch` | Branch metadata only in future versions |
| `Return` | Final `ret` statement uses `op: null` |

## Resource Identity

Mutex / RwLock / condvar / unsafe-var ids in `TransitionType` come from RustPTA’s pointer analysis. **Same id ⇒ same runtime object.** Atomic ops use [`AliasId`](../src/memory/pointsto.rs) string keys in the resource table.

## Limitations

- No concrete values (`unknown` in store/write/cas).
- Branch conditions are not recovered from MIR (`unknown` where applicable).
- Net-only extraction order is a deterministic linearization of transitions per function, not a full partial order.
