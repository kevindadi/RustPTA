#!/usr/bin/env bash
set -euo pipefail

cases_root="${1:-./benchmarks}"
runs_root="${2:-/Users/kevin/local-repos/RustPTA/tmp}"
port="${3:-7878}"

cargo run --bin pn-web -- --cases-root "$cases_root" --runs-root "$runs_root" --port "$port"
