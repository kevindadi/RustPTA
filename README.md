# RustPTA

RustPTA is a Petri-net-based static analyzer for Rust concurrency. It currently focuses on:

- Deadlock detection (`--mode deadlock`)
- Data race detection (`--mode datarace`)
- Atomicity violation detection (`--mode atomic`, requires the `atomic-violation` feature)
- Points-to analysis export (`--mode pointsto`)

## Analysis workflow

1. A Rust compiler callback collects MIR and reachable instances.
2. Build the call graph.
3. Translate MIR to a Petri net.
4. (By default) run Petri net reduction.
5. Build the state graph and run detectors.
6. Emit reports and visualization artifacts.

## Install

```bash
rustup component add rust-src rustc-dev llvm-tools-preview
cargo install --path .
```

## Quick start

### 1) Analyze an entire crate (recommended)

```bash
cargo pn -m deadlock -p your_crate --viz-callgraph --viz-petrinet --viz-stategraph
```

### 2) Analyze a single file

```bash
cargo run --bin pn -- \
  -f path/to/file.rs -m datarace \
  --viz-callgraph --viz-petrinet --viz-stategraph \
  -- path/to/file.rs
```

## Quick validation scripts

```bash
./scripts/check_pn_case.sh deadlock benchmarks/cases/deadlock/dl_1.rs
./scripts/check_pn_case.sh datarace benchmarks/cases/datarace/dr_1.rs
```

Each run also writes `summary.json` under the output directory (for the web UI).

## Web viewer (Rust backend)

```bash
./scripts/run_benchmarks.sh
```

Open `http://127.0.0.1:7878`, pick a run, and inspect:

- `callgraph.dot`
- `petrinet_raw.dot` (shown by default on the home page)
- `petrinet.dot` (final net after reduction)
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `summary.json` and detector reports
- Graph zoom / pan / reset (Fit)
- Recursive scan of run directories under `--root`
- Deadlock reports with readable summaries and deadlock global states (`state_id` + marking)
- Pick a benchmark case and click `Generate` to re-run analysis
- Each `Generate` clears the output directory first (default `/Users/kevin/local-repos/RustPTA/tmp`) to avoid stale results
- `/reduction` shows the three reduction-stage graphs

You can also start it via script:

```bash
./scripts/run_web_viewer.sh ./benchmarks ./tmp 7878
```

## Docker

```bash
docker compose build
docker compose run --rm rustpta cargo pn -m deadlock -p your_crate --pn-analysis-dir ./tmp
```

## Common flags

- `-m, --mode <deadlock|datarace|atomic|all|pointsto>`
- `-p, --pn-crate <name>` — target crate name
- `-f, --file <file.rs>` — single-file mode
- `--pn-analysis-dir <path>` — output root (default `/Users/kevin/local-repos/RustPTA/tmp`)
- `--no-reduce` — disable Petri net reduction
- `--por` — enable partial-order reduction
- `--full` — disable entry-reachable filtering; translate all functions
- `--state-limit <N>` — state-space cap (`0` means unlimited)
- `--stop-after <mir|callgraph|pointsto|petrinet|stategraph>` — stop after a pipeline stage (debugging)

## Output files

Under `<pn-analysis-dir>/<crate_or_file_stem>/`, typical artifacts include:

- `callgraph.dot`
- `petrinet_raw.dot`
- `petrinet.dot`
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `deadlock_report.txt(.json)` / `datarace_report.txt(.json)` / `atomicity_report.txt(.json)`
- `points_to_report.txt` (`pointsto` mode or `--viz-pointsto`)

## Design docs

- Architecture: [`docs/en/01-architecture.md`](./docs/en/01-architecture.md)
- MIR → Petri net: [`docs/en/02-mir-to-petri-net.md`](./docs/en/02-mir-to-petri-net.md)
- Sync primitives: [`docs/en/03-sync-primitives.md`](./docs/en/03-sync-primitives.md)
- Analysis & detection: [`docs/en/04-analysis-detection.md`](./docs/en/04-analysis-detection.md)
- CIR extraction: [`docs/cir_extraction.md`](./docs/cir_extraction.md)
- Known limitations: [`limition.md`](./limition.md)
