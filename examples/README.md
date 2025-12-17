# Tokio 示例项目

这是一个独立的 Cargo 项目，用于测试 RustPTA 的 MIR 输出功能，特别是异步模型的转换。

## 项目结构

```
examples/
├── Cargo.toml          # 独立的项目配置
├── src/
│   └── main.rs         # Tokio 异步示例代码
└── README.md           # 本文件
```

## 运行示例

### 1. 直接运行（正常执行）

```bash
cd examples
cargo run
```

### 2. 使用 RustPTA 分析并输出 MIR

```bash
# 在 examples 目录下
cd examples
cargo clean

# 使用 cargo-pn 分析并输出 MIR
# 注意：crate 名称是 tokio_example
cargo pn -p tokio_example --viz-mir --pn-analysis-dir=./output

# 查看输出的 MIR dot 文件
find ./output -name "*.dot" -type f
```

### 3. 同时输出 MIR 和 Petri 网进行对比

```bash
cargo pn -p tokio_example --viz-mir --viz-petrinet --pn-analysis-dir=./output
```

### 4. 在 Petri 网构建后停止（用于调试）

```bash
cargo pn -p tokio_example --viz-mir --viz-petrinet --stop-after petrinet --pn-analysis-dir=./output
```

## 查看输出

生成的 MIR dot 文件可以使用 Graphviz 工具可视化：

```bash
# 安装 graphviz（如果未安装）
# macOS: brew install graphviz
# Ubuntu: sudo apt-get install graphviz

# 转换为 PNG
dot -Tpng output/tokio_example/mir/<function_name>.dot -o <function_name>.png

# 或者使用在线工具查看 dot 文件
```

## 示例包含的异步模式

1. **async/await**: `async_function` 函数
2. **tokio::spawn**: `test_spawn` 函数
3. **通道通信**: `test_channel` 函数
4. **共享状态**: `test_shared_state` 函数（已定义但未在主函数中调用）

这些模式可以帮助测试异步模型到 Petri 网的转换。

## 注意事项

- 这是一个独立的 Cargo 项目，与主 RustPTA 项目隔离
- 确保 `cargo-pn` 已正确安装并在 PATH 中
- 分析时需要使用正确的 crate 名称：`tokio_example`
