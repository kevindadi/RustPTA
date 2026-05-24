#!/usr/bin/env bash
# Generate cir.yaml for each example under benchmarks/cases into benchmarks/cir/<detector>/<case>.yaml
# Run from repo root; requires `cargo run --bin pn` to work.
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$root/.." && pwd)"
out_root="$root/cir"
# Per-case mktemp; remove work dir after yaml is written to avoid dot/json clutter under tmp
tmp_base="$repo_root/tmp/bench_cir_generated"
mkdir -p "$out_root/deadlock" "$out_root/datarace" "$out_root/atomic"
mkdir -p "$tmp_base"

run_one() {
  local mode="$1"
  local detector="$2"
  local file="$3"
  local stem
  stem=$(basename "$file" .rs)
  local work
  work="$(mktemp -d "${tmp_base}/${detector}_${stem}.XXXXXX")"

  if [[ "$detector" == "atomic" ]]; then
    (cd "$repo_root" && cargo run --features atomic-violation -q --bin pn -- \
      -f "$file" -m "$mode" \
      --pn-analysis-dir "$work" \
      --viz-cir -- \
      "$file") >/dev/null || {
      rm -rf "$work"
      return 1
    }
  else
    (cd "$repo_root" && cargo run -q --bin pn -- \
      -f "$file" -m "$mode" \
      --pn-analysis-dir "$work" \
      --viz-cir -- \
      "$file") >/dev/null || {
      rm -rf "$work"
      return 1
    }
  fi

  local yaml="$work/$stem/cir.yaml"
  if [[ ! -f "$yaml" ]]; then
    echo "error: missing $yaml (from $file)" >&2
    rm -rf "$work"
    return 1
  fi
  cp "$yaml" "$out_root/$detector/${stem}.yaml"
  rm -rf "$work"
  echo "wrote $out_root/$detector/${stem}.yaml"
}

for f in "$root/cases/deadlock"/*.rs; do
  [[ -f "$f" ]] || continue
  run_one deadlock deadlock "$f"
done

for f in "$root/cases/datarace"/*.rs; do
  [[ -f "$f" ]] || continue
  run_one datarace datarace "$f"
done

for f in "$root/cases/atomic"/*.rs; do
  [[ -f "$f" ]] || continue
  run_one atomic atomic "$f"
done

# Remove tmp dir if empty so no stray empty folders remain
rmdir "$tmp_base" 2>/dev/null || true

echo "done: CIR YAML under $out_root"
