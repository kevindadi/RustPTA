# mir2pnml_py

Convert rustc MIR text to PNML Petri net (PTNet, pnml.org 2009 grammar). Control flow + Mutex resource constraints. Python standard library only.

## Quickstart

### 1. Generate MIR text

Use rustc to emit MIR (recommended):

```bash
rustc --emit=mir -Z unpretty=mir your_file.rs
```

This prints MIR to stdout. Redirect to a file:

```bash
rustc --emit=mir -Z unpretty=mir examples/tiny_mutex.rs > examples/tiny_mutex.mir
```

Alternatively, use `-Z dump-mir=main` to write files under `mir_dump/`; concatenate or pick one `.mir` file.

### 2. Run mir2pnml

```bash
python skills/mir2pnml_py/mir2pnml.py --mir examples/tiny_mutex.mir --out out.pnml --dump-json out.json
```

Options:

- `--mir <FILE>` — Input MIR text file (required)
- `--out <FILE>` — Output PNML path (required)
- `--dump-json <FILE>` — Optional: dump internal Petri net as JSON for debugging
- `--entry-fn <NAME>` — Entry function name (default: `main`)
- `--rwlock-n <N>` — RwLock read concurrency token limit (default: 8, reserved)
- `--max-fns <N>` — Max number of functions to parse (default: no limit)

## Supported patterns

- **Mutex::lock**: `callee` contains `Mutex` and `lock` (e.g. `std::sync::Mutex::<T>::lock`, `parking_lot::Mutex::lock`)
- **drop(guard)**: Releases mutex when `guard` is bound via lock/unwrap
- **unwrap/expect propagation**: If `args` contain a local already bound to a mutex, the lhs is bound to the same mutex

## Known limitations

- No alias analysis; mutex key from first local in args (or ref chain `_N = &_M`)
- RwLock, thread spawn/join, atomics not modeled (extension points)
- Cleanup blocks skipped
- MIR format may change; parser targets `-Z unpretty=mir` output

## Extension points

- `--rwlock-n`: reserved for RwLock read tokens
- `pn_builder.py`: add handlers for RwLock, spawn, join, atomic
- `pnml_writer.py`: toolspecific annotations for kind/callee/bb

## Tests

```bash
cd skills/mir2pnml_py && python -m unittest discover tests -v
```
