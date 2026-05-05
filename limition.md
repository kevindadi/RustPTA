# RustPTA: Known limitations (developer guide)

This document summarizes architectural and design constraints in the current RustPTA release for contributors and advanced users. Understanding these limits helps when extending the tool or interpreting results.

## 1. Alias analysis precision vs soundness

### Limitation: resource contention handling tends toward under-approximation

- **Mechanism**: When several objects may alias the same lock or channel, the tool currently connects only the **first** matching candidate.
- **Implications**:
  - **False negatives**: Complex pointer behavior can leave alias relations ambiguous; analyzing only one path may miss deadlocks or races on other paths.
  - **Join semantics**: If a `thread::join` handle may refer to multiple threads, the tool does not build edges for every possibility.

### Possible extension

Strengthen soundness by connecting all ambiguous alias candidates non-deterministically in the graph builder.

## 2. Memory model and atomics

### Limitation: C++11 / Rust memory order is modeled heuristically

- **Mechanism**: Acquire/Release style ordering is approximated via token flow in the Petri net.
- **Implications**:
  - The manual encoding is hard to validate end-to-end.
  - **Relaxed** ordering may be modeled too simply to capture all weak-memory behaviors.

## 3. Control flow

### Limitation: the net is static and finite

- **Recursion**: Unbounded or unpredictable-depth recursion is not supported (the net would grow without a richer formalism).
- **Panic**: Panic paths are connected loosely to function exit; unwinding and drop ordering are not modeled in full detail under complex control flow.

## 4. Foreign Function Interface (FFI)

There is **no** deep analysis of C/C++ code. Concurrency logic outside Rust (across FFI) is invisible to the tool.
