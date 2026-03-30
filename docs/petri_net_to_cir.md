# Petri 网（`net::core::Net`）→ 原生 CIR

本文档描述只读的 **确定性抽取器**：输入为 `crate::net::core::Net`（通过 `crate::net::mod.rs` 的 `pub use` 亦可写作 `net::Net`），输出为可序列化为 YAML 的 `cir::types::CirArtifact`。该格式面向 LLM 与工具链，**不等同**于 `vendor/cir`（CEIR）里的 `res_op` JSON 形状。

## 架构

```text
Net
  └── CirExtractor::extract()
        ├── Phase 1  extract_resources      (Lock / RwLock / Notify / Atomics / Unsafe*)
        ├── Phase 2  resolve_condvar_pairing (Wait ↔ Mutex / Condvar)
        ├── Phase 3  discover_functions    (FunctionStart / FunctionEnd / 控制子图)
        ├── Phase 4  linearize_function     (最短控制路径上的语义变迁)
        ├── Phase 5  infer_protection       (held 锁集合 ∩)
        ├── Phase 6  generate_goals         (main + Spawn 目标)
        └── Phase 7  anchor_map             (sid ↔ place / transition, resource ↔ places)
```

## 与 CEIR / 既有 JSON 的差异

| 方面 | 原生 CIR（本模块） | CEIR 示例 JSON |
|------|-------------------|----------------|
| 操作形状 | `serde` untagged：`lock:` / `drop:` / `wait:` 等键 | 常为 `res_op` 元组列表 |
| 分支 | `CirTransfer::Branch`（Switch 两路输出时；当前 MIR 网若多于两路控制出边会报 `AmbiguousBranch`） | 依 CEIR 方言 |
| 来源 | 仅从 `Net` 只读推导 | 常自高层 IR 或手写 |

## 变迁类型映射（节选）

`TransitionType` 在 `src/net/structure.rs`。抽取器当前**不会**为未列出的类型生成 CIR 算子（多数被跳过或折叠进控制边）。

| `TransitionType` | CIR（若生成） |
|------------------|---------------|
| `Lock(r)` | `lock: m{i}` |
| `Unlock` / `DropRead` / `DropWrite` | `drop:` |
| `RwLockRead` / `RwLockWrite` | `read_lock` / `write_lock` |
| `Wait` | `wait: { cv, mutex }`（需与 `Notify`、Mutex 库所拓扑一致） |
| `Notify(r)` | `notify_one` |
| `UnsafeRead` / `UnsafeWrite` | `read` / `write` |
| `AtomicLoad` / `AtomicStore` / `AtomicCmpXchg` | `load` / `store` / `cas` |
| `Spawn` / `Join` | `spawn` / `join` |
| `Goto` / `Normal` / `Return` | 不单独生成语句（控制压缩） |

## 已知缺口

- **信号量 / 有界 Channel**：当前 `TransitionType` **没有**独立的 `SemAcquire` / `ChannelSend` 等；`ResourceKind::Semaphore` / `Channel` 便于 YAML 扩展，**默认不会**从现网导出对应变迁。
- **`Switch` → `Branch`**：实现期望**恰好两条**非资源控制出边；否则产生 `AmbiguousBranch`（见 `ExtractionError`）。
- **条件文本**：分支条件初版可能为变迁名或占位符，与 MIR 表达式不对一。

## 弧与标识

- 使用 `net.pre.get(place, transition)` / `net.post.get(place, transition)`（`Incidence`），而非规范里假想的 `pre[place][t]` 下标形式。
- 库所 ID：`PlaceId`（`u32` newtype）；锚点中可用 `.index()` 得到 `usize`。

## 函数与基本块命名

与 `translate/petri_net.rs` / `mir_to_pn` 一致的方向：

- 函数：`{path_or_name}_start` / `{same_key}_end`；`CirArtifact.functions` 的键为路径**最后一段**（如 `test_mod::thread_a_start` → `thread_a`）。
- 基本块：`{body}_{idx}` 形式有利于与 PN 一致（见 `terminator.rs` 中的格式化）。

## `ExtractionError` 摘要

| 变体 | 含义 |
|------|------|
| `NoEntryFunction` | 无带初始令牌的 `FunctionStart`（或未发现合法函数） |
| `UnpairedCondvar` | `Wait` 无法与 Mutex / Condvar _rid 稳定一致配对 |
| `DisconnectedControlFlow` | 控制子图不连通（若有检查） |
| `AmbiguousBranch` | `Switch` 非两路控制输出 |
| `MissingEnd` | 存在 `FunctionStart` 而无匹配的 `{key}_end` |

## 用法示例

```rust
use RustPTA::cir::CirExtractor;
use RustPTA::net::core::Net;

fn dump(net: &Net) {
    let artifact = CirExtractor::new(net).extract().expect("extract");
    println!("{}", artifact.to_yaml().unwrap());
}
```

**差异比对**（期望 CIR vs 抽取结果）：

```rust
use RustPTA::cir::{CirArtifact, CirDiff, extract_and_verify};

let res = extract_and_verify(&net, Some(&expected));
assert!(res.extracted.is_some());
let diff = res.diff.as_ref().unwrap();
// `is_conformant`: 允许「多出」资源/函数；不得缺少期望中的资源/函数，且 protection/goals 须一致
```

## 集成测试

`tests/cir_extractor_tests.rs` 使用 `Net::empty`、`add_place`、`add_transition`、`add_input_arc`、`add_output_arc` 构建小型网，覆盖 YAML 往返、互斥、读写锁、`Wait`/`Notify`、保护推断、`Spawn`/目标与锚点等场景。
