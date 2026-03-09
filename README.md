# RustPTA

RustPTA 是一个基于 Petri 网的 Rust 并发静态分析工具，当前重点支持：
- 死锁检测（`--mode deadlock`）
- 数据竞争检测（`--mode datarace`）
- 原子性违背检测（`--mode atomic`，需 `atomic-violation` feature）
- 指针分析导出（`--mode pointsto`）

## 分析工作流

1. Rust 编译器回调收集 MIR 与可达实例。
2. 构建调用图（Call Graph）。
3. MIR 翻译为 Petri 网。
4. （默认）执行 Petri 网缩减。
5. 构建状态图并执行检测器。
6. 输出报告与可视化文件。

## 安装

```bash
rustup component add rust-src rustc-dev llvm-tools-preview
cargo install --path .
```

## 快速使用

### 1) 分析整个 crate（推荐）

```bash
cargo pn -m deadlock -p your_crate --pn-analysis-dir ./tmp --viz-callgraph --viz-petrinet --viz-stategraph
```

### 2) 分析单个文件

```bash
cargo run --bin pn -- \
  -f path/to/file.rs -m datarace --pn-analysis-dir ./tmp \
  --viz-callgraph --viz-petrinet --viz-stategraph \
  -- path/to/file.rs
```

macOS 若遇到链接环境问题，可在 `--` 后追加 `-Zno-link`。

## 常用参数

- `-m, --mode <deadlock|datarace|atomic|all|pointsto>`
- `-p, --pn-crate <name>`：目标 crate 名
- `-f, --file <file.rs>`：单文件模式
- `--pn-analysis-dir <path>`：输出根目录（默认 `./tmp`）
- `--no-reduce`：关闭 Petri 网缩减
- `--por`：开启部分序约简
- `--full`：关闭入口可达过滤，翻译全部函数
- `--state-limit <N>`：状态空间上限（0 表示不限制）
- `--stop-after <mir|callgraph|pointsto|petrinet|stategraph>`：调试分阶段停止

## 输出文件

输出目录为 `<pn-analysis-dir>/<crate_or_file_stem>/`，常见文件：
- `callgraph.dot`
- `petrinet.dot`
- `stategraph.dot`
- `deadlock_report.txt(.json)` / `datarace_report.txt(.json)` / `atomicity_report.txt(.json)`
- `points_to_report.txt`（`pointsto` 模式或 `--viz-pointsto`）

## 已知限制

见 [limition.md](./limition.md)。
