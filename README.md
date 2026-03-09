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
cargo pn -m deadlock -p your_crate --viz-callgraph --viz-petrinet --viz-stategraph
```

### 2) 分析单个文件

```bash
cargo run --bin pn -- \
  -f path/to/file.rs -m datarace \
  --viz-callgraph --viz-petrinet --viz-stategraph \
  -- path/to/file.rs
```

## 快速验证脚本

```bash
./scripts/check_pn_case.sh deadlock benchmarks/cases/deadlock/dl_1.rs
./scripts/check_pn_case.sh datarace benchmarks/cases/datarace/dr_1.rs
```

每次分析会在结果目录额外产出 `summary.json`（供前端读取）。

## Web Viewer（Rust 后端）

```bash
cargo run --bin pn-web -- --cases-root ./benchmarks --runs-root /Users/kevin/local-repos/RustPTA/tmp --port 7878
```

打开 `http://127.0.0.1:7878`，可选择某次运行并查看：

- `callgraph.dot`
- `petrinet_raw.dot`（首页默认显示）
- `petrinet.dot`（约减后最终图）
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `summary.json` 与检测报告
- 支持图缩放/拖拽/重置（Fit）
- 支持递归扫描 `--root` 下的运行目录
- 死锁报告会显示更易读摘要与死锁状态位置（`state_id` + marking）
- 页面可直接选择 `benchmark` 下 case 并点击 `Generate` 重新生成结果
- 每次 `Generate` 前会先清空输出目录（默认 `/Users/kevin/local-repos/RustPTA/tmp`）防止脏结果
- 新增 `/reduction` 页面专门查看三次约减流程图

也可用脚本启动：

```bash
./scripts/run_web_viewer.sh ./benchmarks /Users/kevin/local-repos/RustPTA/tmp 7878
```

## Docker 使用

```bash
docker compose build
docker compose run --rm rustpta cargo pn -m deadlock -p your_crate --pn-analysis-dir /Users/kevin/local-repos/RustPTA/tmp
```

## 常用参数

- `-m, --mode <deadlock|datarace|atomic|all|pointsto>`
- `-p, --pn-crate <name>`：目标 crate 名
- `-f, --file <file.rs>`：单文件模式
- `--pn-analysis-dir <path>`：输出根目录（默认 `/Users/kevin/local-repos/RustPTA/tmp`）
- `--no-reduce`：关闭 Petri 网缩减
- `--por`：开启部分序约简
- `--full`：关闭入口可达过滤，翻译全部函数
- `--state-limit <N>`：状态空间上限（0 表示不限制）
- `--stop-after <mir|callgraph|pointsto|petrinet|stategraph>`：调试分阶段停止

## 输出文件

输出目录为 `<pn-analysis-dir>/<crate_or_file_stem>/`，常见文件：

- `callgraph.dot`
- `petrinet_raw.dot`
- `petrinet.dot`
- `petrinet_reduce_1_loop.dot`
- `petrinet_reduce_2_sequence.dot`
- `petrinet_reduce_3_intermediate.dot`
- `stategraph.dot`
- `deadlock_report.txt(.json)` / `datarace_report.txt(.json)` / `atomicity_report.txt(.json)`
- `points_to_report.txt`（`pointsto` 模式或 `--viz-pointsto`）

## 设计与规划文档

- 验证与前端规划：[`docs/VALIDATION_UI_PLAN.md`](./docs/VALIDATION_UI_PLAN.md)
- 已知限制：[`limition.md`](./limition.md)
