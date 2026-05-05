#!/usr/bin/env bash
# Run the full benchmark suite from the repo root: for each case run `pn` with and
# without reduction; aggregate call-graph / net / state-graph sizes and bug counts
# into benchmarks/results/.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v rg >/dev/null 2>&1; then
  echo "ripgrep (rg) is required to parse DOT files. Example: brew install ripgrep" >&2
  exit 1
fi

cd "$repo_root"
exec "$repo_root/benchmarks/run_benchmarks.sh"
