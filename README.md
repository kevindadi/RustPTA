# Rust 死锁和数据竞争检测工具使用说明

面向 Rust 程序的静态死锁检测工具，可检测的死锁类型包括双锁、冲突锁以及与条件变量相关的死锁。

## 1. 安装与使用
仅支持Linux：当前使用的 Rust 编译器版本为 `nightly-2023-09-13`。为了进行死锁检测，首先需要安装必要的工具链和相关依赖，然后执行 `cargo pta` 命令。以下是具体的操作步骤：

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
3. 运行以下命令，对待检测文件进行死锁检测：

   `cd /path/crate; cargo clean; cargo pta`
4. 使用 Petri 网检测死锁（需指定待检测包名，以缩减状态空间）：

   `cd /path/crate && cargo clean && cargo pta --main_crate crate`
5. 基于 sanitizer 检测数据竞争

   `cd /path/crate && cargo clean && cargo pta --detector_kind datarace`

## 2. 检测结果

### 2.1 基于锁图的检测工具报告说明

检测结果输出包括死锁类型、产生死锁的原因、涉及的变量信息（包括类型和源代码位置），以及产生死锁的相关语句的调用上下文信息。以下是检测结果输出的示例，展示了一个双锁错误：

```json
{
  "DoubleLock": {
    "bug_kind": "DoubleLock",
    "diagnosis": {
      "first_lock_type": "StdMutex(i32)",
      "first_lock_span": "src/main.rs:8:9: 8:15 (#0)",
      "second_lock_type": "StdMutex(i32)",
      "second_lock_span": "src/main.rs:16:9: 16:15 (#0)",
      "callchains": [
        ["src/main.rs:10:5: 10:19 (#0)"]
      ]
    },
    "explanation": "The first lock is not released when acquiring the second lock"
  }
}

```

### 2.2 基于 Petri 网的检测工具报告说明
以 `example/inter` 为例，检测结果为死锁发生的当前程序状态，下表第三个状态示例如下，表示程序处于`Foo::sync_mutex_2bb2:1span: src/main.rs:35:10: 35:34 (#0)`: sync_mutex_2(), `mainbb1wait:1span: src/main.rs:162:5: 162:24 (#0)`: sync_mutex_1() 和 `Foo::sync_mutex_1bb5wait:1span: src/main.rs:29:17: 29:36 (#0)`: main() 处发生死锁，其他标识为当前状态下的资源, 即程序处于main()->sync_mutex_1()->sync_mutex_2();
注： 如果程序不可达，那么Petri网将不会识别到死锁，修复死锁后可重新验证。`example/inter`示例中函数`foo1.sync_rwlock_write_1()`后不可达，故只有四个死锁。
```
deadlock state: "rwlock1":9span: , "mutex0":1span: , Foo::sync_rwlock_write_2bb1:1span: src/main.rs:65:10: 65:35 (#0), Foo::sync_rwlock_read_1bb5wait:1span: src/main.rs:41:17: 41:43 (#0), mainbb3wait:1span: src/main.rs:164:5: 164:30 (#0)

"mutex0":1span: , Foo::sync_rwlock_read_2bb1:1span: src/main.rs:61:18: 61:42 (#0), mainbb4wait:1span: src/main.rs:165:5: 165:31 (#0), Foo::sync_rwlock_write_1bb4wait:1span: src/main.rs:55:17: 55:42 (#0)

"rwlock1":10span: , Foo::sync_mutex_1bb5wait:1span: src/main.rs:29:17: 29:36 (#0), Foo::sync_mutex_2bb2:1span: src/main.rs:35:10: 35:34 (#0), mainbb1wait:1span: src/main.rs:162:5: 162:24 (#0)

"mutex0":1span: , Foo::sync_rwlock_write_2bb1:1span: src/main.rs:65:10: 65:35 (#0), mainbb4wait:1span: src/main.rs:165:5: 165:31 (#0), Foo::sync_rwlock_write_1bb5wait:1span: src/main.rs:52:17: 52:43 (#0)

```

### 2.3 数据竞争检测结果
关于数据竞争，给出竞态条件发生的位置，和先后的竞态操作。例如：
```
Data Race Report:
Location: main.rs:6->data_race::main
main thread write in main.rs:8--->thread T1 Write in main.rs:6
```

