#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 2 ]; then
  echo "Usage: $0 <mode> <rust-file> [output-dir]" >&2
  exit 1
fi

mode="$1"
case_file="$2"
out_root="${3:-/Users/kevin/local-repos/RustPTA/tmp}"

cargo run --bin pn -- \
  -f "$case_file" -m "$mode" \
  --pn-analysis-dir "$out_root" \
  --viz-callgraph --viz-petrinet --viz-stategraph \
  -- "$case_file"

case_name="$(basename "$case_file" .rs)"
out_dir="$out_root/$case_name"

for f in callgraph.dot petrinet.dot stategraph.dot; do
  if [ ! -s "$out_dir/$f" ]; then
    echo "Missing or empty artifact: $out_dir/$f" >&2
    exit 2
  fi
done

if [ "$mode" = "deadlock" ] && [ ! -s "$out_dir/deadlock_report.txt.json" ]; then
  echo "Missing deadlock report json" >&2
  exit 3
fi

if [ "$mode" = "datarace" ] && [ ! -s "$out_dir/datarace_report.txt.json" ]; then
  echo "Missing datarace report json" >&2
  exit 4
fi

if [ "$mode" = "atomic" ] && [ ! -s "$out_dir/atomicity_report.txt.json" ]; then
  echo "Missing atomic report json" >&2
  exit 5
fi

echo "OK: artifacts generated at $out_dir"
