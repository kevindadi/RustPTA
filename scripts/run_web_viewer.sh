#!/usr/bin/env bash
set -euo pipefail

cases_root="${1:-./benchmarks/cases}"
runs_root="${2:-./tmp/web}"
port="${3:-7878}"

cargo run --bin pn-web -- --cases-root "$cases_root" --runs-root "$runs_root" --port "$port"
