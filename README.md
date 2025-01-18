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
        --pn-analysis-output=<PATH>            Output path for analysis results [default: diagnostics.json]
            --type <TYPE>              Target crate type (binary/library) [default: binary]
            --api-spec <PATH>          Path to library API specification file
        --pn-test                      Do not perform state reduction

    VISUALIZATION OPTIONS:
            --viz-callgraph            Generate call graph visualization
            --viz-petrinet            Generate Petri net visualization
            --viz-stategraph          Generate state graph visualization
            --viz-unsafe              Generate unsafe operations report
            --viz-pointsto 

    EXAMPLES:
        cargo pn -m deadlock -t my_crate --pn-analysis-dir=./tmp --viz-petrinet
    "#;

# 2. 建模方法
该工具对每个实例化函数的控制流一一对应建模，以 example/double_lock为例，其对应的中间表示可以在`https://play.rust-lang.org/`找到。
```bash
fn main() -> () {
    let mut _0: ();
    let _1: std::sync::Arc<std::sync::Mutex<i32>>;
    let mut _2: std::sync::Mutex<i32>;
    let mut _4: &std::sync::Arc<std::sync::Mutex<i32>>;
    let mut _6: std::result::Result<std::sync::MutexGuard<'_, i32>, std::sync::PoisonError<std::sync::MutexGuard<'_, i32>>>;
    let _7: &std::sync::Mutex<i32>;
    let mut _8: &std::sync::Arc<std::sync::Mutex<i32>>;
    let mut _10: {closure@src/main.rs:9:29: 9:36};
    let _11: ();
    let mut _12: std::result::Result<(), std::boxed::Box<dyn std::any::Any + std::marker::Send>>;
    let mut _13: bool;
    scope 1 {
        debug mu1 => _1;
        let _3: std::sync::Arc<std::sync::Mutex<i32>>;
        scope 2 {
            debug mu2 => _3;
            let _5: std::sync::MutexGuard<'_, i32>;
            scope 3 {
                debug g1 => _5;
                let _9: std::thread::JoinHandle<()>;
                scope 4 {
                    debug th1 => _9;
                }
            }
        }
    }

    bb0: {
        _13 = const false;
        _2 = Mutex::<i32>::new(const 1_i32) -> [return: bb1, unwind continue];
    }

    bb1: {
        _1 = Arc::<Mutex<i32>>::new(move _2) -> [return: bb2, unwind continue];
    }

    bb2: {
        _4 = &_1;
        _3 = <Arc<Mutex<i32>> as Clone>::clone(move _4) -> [return: bb3, unwind: bb13];
    }

    bb3: {
        _13 = const true;
        _8 = &_1;
        _7 = <Arc<Mutex<i32>> as Deref>::deref(move _8) -> [return: bb4, unwind: bb16];
    }

    bb4: {
        _6 = Mutex::<i32>::lock(copy _7) -> [return: bb5, unwind: bb16];
    }

    bb5: {
        _5 = Result::<MutexGuard<'_, i32>, PoisonError<MutexGuard<'_, i32>>>::unwrap(move _6) -> [return: bb6, unwind: bb16];
    }

    bb6: {
        _13 = const false;
        _10 = {closure@src/main.rs:9:29: 9:36} { mu2: move _3 };
        _9 = spawn::<{closure@src/main.rs:9:29: 9:36}, ()>(move _10) -> [return: bb7, unwind: bb12];
    }

    bb7: {
        _12 = JoinHandle::<()>::join(move _9) -> [return: bb8, unwind: bb12];
    }

    bb8: {
        _11 = Result::<(), Box<dyn Any + Send>>::unwrap(move _12) -> [return: bb9, unwind: bb12];
    }

    bb9: {
        drop(_5) -> [return: bb10, unwind: bb16];
    }

    bb10: {
        _13 = const false;
        drop(_1) -> [return: bb11, unwind continue];
    }

    bb11: {
        return;
    }

    bb12 (cleanup): {
        drop(_5) -> [return: bb16, unwind terminate(cleanup)];
    }

    bb13 (cleanup): {
        drop(_1) -> [return: bb14, unwind terminate(cleanup)];
    }

    bb14 (cleanup): {
        resume;
    }

    bb15 (cleanup): {
        drop(_3) -> [return: bb13, unwind terminate(cleanup)];
    }

    bb16 (cleanup): {
        switchInt(copy _13) -> [0: bb13, otherwise: bb15];
    }
}

fn main::{closure#0}(_1: {closure@src/main.rs:9:29: 9:36}) -> () {
    debug mu2 => (_1.0: std::sync::Arc<std::sync::Mutex<i32>>);
    let mut _0: ();
    let mut _2: std::sync::MutexGuard<'_, i32>;
    let mut _3: std::result::Result<std::sync::MutexGuard<'_, i32>, std::sync::PoisonError<std::sync::MutexGuard<'_, i32>>>;
    let _4: &std::sync::Mutex<i32>;
    let mut _5: &std::sync::Arc<std::sync::Mutex<i32>>;
    let mut _6: &mut i32;
    let mut _7: &mut std::sync::MutexGuard<'_, i32>;
    scope 1 {
        debug g2 => _2;
    }

    bb0: {
        _5 = &(_1.0: std::sync::Arc<std::sync::Mutex<i32>>);
        _4 = <Arc<Mutex<i32>> as Deref>::deref(move _5) -> [return: bb1, unwind: bb8];
    }

    bb1: {
        _3 = Mutex::<i32>::lock(copy _4) -> [return: bb2, unwind: bb8];
    }

    bb2: {
        _2 = Result::<MutexGuard<'_, i32>, PoisonError<MutexGuard<'_, i32>>>::unwrap(move _3) -> [return: bb3, unwind: bb8];
    }

    bb3: {
        _7 = &mut _2;
        _6 = <MutexGuard<'_, i32> as DerefMut>::deref_mut(move _7) -> [return: bb4, unwind: bb7];
    }

    bb4: {
        (*_6) = const 2_i32;
        drop(_2) -> [return: bb5, unwind: bb8];
    }

    bb5: {
        drop(_1) -> [return: bb6, unwind continue];
    }

    bb6: {
        return;
    }

    bb7 (cleanup): {
        drop(_2) -> [return: bb8, unwind terminate(cleanup)];
    }

    bb8 (cleanup): {
        drop(_1) -> [return: bb9, unwind terminate(cleanup)];
    }

    bb9 (cleanup): {
        resume;
    }
}
```
对应的网模型为：
![原始Petri 网模型](/home/kevin/RustPTA/example/double_lock/tmp1/double_lock/graph.png "初始Petri网")
经过状态缩减后的模型为：
![Petri 网模型](/home/kevin/RustPTA/example/double_lock/tmp/double_lock/graph.png "Petri网")

# 3. 运行实例与结果说明
## 3.1 死锁检测(以example/condvar_lock为例)
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


## 3.2 数据竞争检测(以example/address_reuse为例)
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


## 3.3 原子性违背检测(以example/atomic_se为例)
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

## 3.4 万行级代码检测
以 example/thousands_code为例，在定义 entry 函数后，（bin 文件默认为 main 函数，lib 库默认需要自行指定），工具只对调用链上的函数进行建模，所以函数库大小与模型并不强相关，建模工具可以在可接受时间内结束。
