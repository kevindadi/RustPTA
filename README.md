# Rust 死锁和数据竞争检测工具使用说明

面向 Rust 程序的静态死锁检测工具，可检测的死锁类型包括双锁、冲突锁以及与条件变量相关的死锁。

## 1. 安装与使用（仅在Linux下测试）

1. 下载工具代码并进入 RustPTA 目录。
2. 运行以下命令，安装必要的工具链和相关依赖：
  ```bash
   sudo apt-get install gcc g++ clang llvm
  ```

  ```bash
   rustup component add rust-src
   rustup component add rustc-dev
   rustup component add llvm-tools-preview
   cargo install --path .
  ```
3. Usage
   `cargo pn -- -m datarace -t my_crate --type library --api-spec tests/lib.toml--viz-callgraph --viz-petrinet`

