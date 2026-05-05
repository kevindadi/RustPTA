#!/usr/bin/env bash
# 从仓库根目录跑完整 benchmark 套件：对每个 case 执行 pn（约简 / 非约简），
# 汇总调用图、Petri 网、状态图规模与 bug 计数到 benchmarks/results/。
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v rg >/dev/null 2>&1; then
  echo "需要 ripgrep (rg) 才能解析 DOT。安装示例: brew install ripgrep" >&2
  exit 1
fi

cd "$repo_root"
exec "$repo_root/benchmarks/run_benchmarks.sh"
