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
3. 基于锁图的检测方式:
  `cd path/to/your/rust/project; cargo clean; cargo pta`

4. 基于Petri网的检测方式:
  `cd path/to/your/rust/project; cargo clean;`
   ```c 
   const CARGO_PN_HELP: &str = r#"Petri Net-based Analysis Tool for Rust Programs

    USAGE:
        cargo pn [OPTIONS] [-- <rustc-args>...]

    OPTIONS:
        -h, --help                      Print help information
        -V, --version                   Print version information
        -m, --mode <TYPE>              Analysis mode:
                                      - deadlock: Deadlock detection
                                      - datarace: Data race detection
                                      - memory: Memory safety analysis
                                      - [default: deadlock]
        -t, --target <NAME>            Target crate for analysis(Only underlined links can be used)
        -pn-analysis-output=<PATH>            Output path for analysis results [default: diagnostics.json]
            --type <TYPE>              Target crate type (binary/library) [default: binary]
            --api-spec <PATH>          Path to library API specification file

    VISUALIZATION OPTIONS:
            --viz-callgraph            Generate call graph visualization
            --viz-petrinet            Generate Petri net visualization
            --viz-stategraph          Generate state graph visualization
            --viz-unsafe              Generate unsafe operations report
            --viz-pointsto 

    EXAMPLES:
        cargo pn -m datarace -t my_crate
        cargo pn -m all -o results.json --viz-petrinet
        cargo pn -t my_lib --type library --api-spec apis.json
    "#;


# 2. 运行实例与结果说明
## 2.1 死锁检测(以example/condvar_lock为例)
基于Petri网的检测结果表示为程序当前所处的（状态），以及该状态所包含的资源和所对应的源代码位置。如以下结果：
```bash 
    分析工具: Petri Net Deadlock Detector
    分析时间: 6.644µs
    是否存在死锁: true
    
    发现 2 个死锁状态:
    
    死锁 #1
    状态ID: s105
    描述: Deadlock state with blocked resources
    标识:
      Condvar:src/main.rs:11:46: 11:60 (#0) (): 1
      incorrect_use_condvar::{closure#0}_2 (src/main.rs:14:18: 14:37 (#0)): 1
      main_0_wait (src/main.rs:5:5: 5:28 (#0)): 1
      incorrect_use_condvar_16 (src/main.rs:30:5: 30:15 (#0)): 1
    
    死锁 #2
    状态ID: s100
    描述: Deadlock state with blocked resources
    标识:
      incorrect_use_condvar::{closure#0}_11 (src/main.rs:19:23: 19:50 (#0)): 1
      main_0_wait (src/main.rs:5:5: 5:28 (#0)): 1
      incorrect_use_condvar_10 (src/main.rs:23:14: 23:33 (#0)): 1
```

结果描述：
死锁1表明程序当前处于状态s105，该状态包含资源Condvar、incorrect_use_condvar::{closure#0}_2、main_0_wait、incorrect_use_condvar_16，这些资源对应的源代码位置分别为src/main.rs:11:46: 11:60 (#0)、src/main.rs:14:18: 14:37 (#0)、src/main.rs:5:5: 5:28 (#0)、src/main.rs:30:5: 30:15 (#0)。
main_0_wait表示为main函数等待调用函数的返回，incorrect_use_condvar::{closure#0}_2表示为incorrect_use_condvar函数所产生的闭包所阻塞的位置，Condvar表示为条件变量资源，incorrect_use_condvar_16表示为incorrect_use_condvar函数所阻塞的位置。
对应的程序行为是main函数调用incorrect_use_condvar函数，incorrect_use_condvar函数阻塞等待闭包的返回，而此时incorrect_use_condvar持有锁mu1，导致闭包阻塞在14行。

死锁2与之同理，不过阻塞行为不同，死锁2在闭包获取锁后，等待信号量，而incorrect_use_condvar函数在23行阻塞，无法通知闭包。


## 2.2 数据竞争检测(以example/address_reuse为例)
```bash
[2025-01-17T22:31:53Z INFO  pn::callback] Race Ok("{\"unsafe_transitions\":[70,82]}"):
    {
      "operations": [
        "(Write)-->thread2_write_PlaceTy { ty: &'{erased} std::cell::SyncUnsafeCell<i32>, variant_index: None }_in:src/main.rs:40:17: 40:20 (#0)",
        "(Write)-->thread1_write_PlaceTy { ty: &'{erased} std::cell::SyncUnsafeCell<i32>, variant_index: None }_in:src/main.rs:22:9: 22:12 (#0)"
      ]
    }
[2025-01-17T22:31:53Z INFO  pn::callback] Race Ok("{\"unsafe_transitions\":[74,82]}"):
    {
      "operations": [
        "(Read)-->thread2_read_PlaceTy { ty: i32, variant_index: None }_in:/home/kevin/.rustup/toolchains/nightly-2024-12-11-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/macros/mod.rs:44:16: 44:22 (#15)",
        "(Write)-->thread1_write_PlaceTy { ty: &'{erased} std::cell::SyncUnsafeCell<i32>, variant_index: None }_in:src/main.rs:22:9: 22:12 (#0)"
      ]
    }
```

结果描述，数据竞争主要对于static数据和对于实现Sync的类型在unsafe代码中操作导致的读写不一致或写冲突进行检测。


## 2.3 原子性违背检测(以example/atomic_se为例)
```bash
分析工具: Petri Net Atomicity Violation Detector
    分析时间: 362.735µs
    是否存在违背: true
    
    发现 3 个原子性违背模式:
    
    违背模式 #1:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:16:9: 16:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
    
    违背模式 #2:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:16:9: 16:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:22:9: 22:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      3. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
    
    违背模式 #3:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:22:9: 22:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
```

结果描述，对于原子性违背，主要检测在load操作时，是否存在不同线程或同一个线程内的两个store操作，导致在对load结果判断时，出现未定义的行为。