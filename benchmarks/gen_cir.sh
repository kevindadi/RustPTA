#!/usr/bin/env bash
# 为 benchmarks/cases 下各示例生成对应的 cir.yaml，写入 benchmarks/cir/<detector>/<case>.yaml
# 依赖：在仓库根目录执行；需已能 `cargo run --bin pn`。
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$root/.." && pwd)"
out_root="$root/cir"
# 每次用例单独 mktemp，成功写出 yaml 后立即删掉，避免 tmp 下堆积 dot/json 等中间文件
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

# 若目录已空则删掉，避免残留空文件夹
rmdir "$tmp_base" 2>/dev/null || true

echo "done: CIR YAML under $out_root"
