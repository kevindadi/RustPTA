# RustPTA

RustPTA is a Petri-net-based static analyzer for Rust concurrency. It currently focuses on:

- Deadlock detection (`--mode deadlock`)
- Data race detection (`--mode datarace`)
- Atomicity violation detection (`--mode atomic`, requires `atomic-violation` feature)
- Pointer-analysis export (`--mode pointsto`)

## Analysis pipeline

1. Compiler hooks collect MIR and reachable monomorphized instances.
2. Build the call graph.
3. Translate MIR into a Petri net.
4. (By default) apply Petri net reduction.
5. Build the state graph and run detectors.
6. Emit reports and visualization artifacts.

## Installation

```bash
rustup component add rust-src rustc-dev llvm-tools-preview
cargo install --path .
```

## Quick start

### 1) Analyze a whole crate (recommended)

```bash
cargo pn -m deadlock -p your_crate --viz-callgraph --viz-petrinet --viz-stategraph
```

### 2) Single-file mode

```bash
cargo run --bin pn -- \
  -f path/to/file.rs -m datarace \
  --viz-callgraph --viz-petrinet --viz-stategraph \
  -- path/to/file.rs
```

### Smoke-test scripts

```bash
./scripts/check_pn_case.sh deadlock benchmarks/cases/deadlock/dl_1.rs
./scripts/check_pn_case.sh datarace benchmarks/cases/datarace/dr_1.rs
```

Each run also writes `summary.json` under the output directory for the web UI.

### Benchmark suite

From the repository root:

```bash
./scripts/run_benchmarks.sh
```

See `benchmarks/run_benchmarks.sh` for details (requires `rg`). Outputs go under `benchmarks/results/`.

## Web viewer (Rust backend)

```bash
cargo run --bin pn-web -- --cases-root ./benchmarks --runs-root ./tmp --port 7878
```

Open `http://127.0.0.1:7878` to browse a run and inspect:

- `callgraph.dot`
- `petrinet_raw.dot` (default on the home view)
- `petrinet.dot` (reduced net)
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `summary.json` and detector reports
- Pan/zoom/drag/reset (Fit)
- Recursive scans of runs under `--root`
- Deadlock reports with readable summaries and state locations (`state_id` + marking)
- Pick a benchmark case and **Generate** to re-run analysis
- The output directory is cleared before each **Generate** (override with `--runs-root`, default in examples is often `./tmp`)
- `/reduction` shows the three reduction stages

Alternative launcher:

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
- `--pn-analysis-dir <path>` — output root (tool default may be set in `options`; use an explicit path in CI)
- `--no-reduce` — disable Petri net reduction
- `--por` — enable partial-order reduction
- `--full` — disable entry-reachability filtering; translate all functions
- `--state-limit <N>` — state-space cap (`0` = no limit)
- `--stop-after <mir|callgraph|pointsto|petrinet|stategraph>` — stop the pipeline early for debugging

## Output files

Under `<pn-analysis-dir>/<crate_or_file_stem>/` you typically get:

- `callgraph.dot`
- `petrinet_raw.dot`
- `petrinet.dot`
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `deadlock_report.txt(.json)` / `datarace_report.txt(.json)` / `atomicity_report.txt(.json)`
- `points_to_report.txt` (`pointsto` mode or `--viz-pointsto`)

## Documentation

- [Architecture and analysis pipeline](docs/01-architecture.md)
- [MIR to Petri net mapping](docs/02-mir-to-petri-net.md)
- [Synchronization primitives (Petri net models)](docs/03-sync-primitives.md)
- [Pointer analysis and bug detection](docs/04-analysis-detection.md)
- [Known limitations](limition.md)
