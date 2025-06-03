# Rust Deadlock and Race Condition Detection Tool Usage Guide

A static deadlock detection tool for Rust programs that can detect various types of deadlocks including double locks, conflicting locks, and deadlocks related to condition variables.

## 1. Installation and Usage (Tested only on Linux)

1. Download the tool code and enter the RustPTA directory.
2. Run the following commands to install necessary toolchain and dependencies:
  ```bash
   sudo apt-get install gcc g++ clang llvm
  ```

  ```bash
   rustup component add rust-src
   rustup component add rustc-dev
   rustup component add llvm-tools-preview
   cargo install --path .
  ```
3. Lock graph-based detection method:
  `cd path/to/your/rust/project; cargo clean; cargo pta`

4. Petri net-based detection method:
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

# 2. Modeling Method
This tool models the control flow of each instantiated function one-to-one. Taking example/double_lock as an example, its corresponding intermediate representation can be found at `https://play.rust-lang.org/`.
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
The corresponding network model is:
![Original Petri Net Model](/home/kevin/RustPTA/example/double_lock/tmp1/double_lock/graph.png "Initial Petri Net")
The model after state reduction is:
![Petri Net Model](/home/kevin/RustPTA/example/double_lock/tmp/double_lock/graph.png "Petri Net")

# 3. Running Examples and Result Explanation
## 3.1 Deadlock Detection (using example/condvar_lock as example)
Petri net-based detection results are represented as the current state of the program, including the resources contained in that state and the corresponding source code locations. For example, the following result:
```bash 
    Analysis Tool: Petri Net Deadlock Detector
    Analysis Time: 6.644µs
    Deadlock Found: true
    
    Found 2 deadlock states:
    
    Deadlock #1
    State ID: s105
    Description: Deadlock state with blocked resources
    Tokens:
      Condvar:src/main.rs:11:46: 11:60 (#0) (): 1
      incorrect_use_condvar::{closure#0}_2 (src/main.rs:14:18: 14:37 (#0)): 1
      main_0_wait (src/main.rs:5:5: 5:28 (#0)): 1
      incorrect_use_condvar_16 (src/main.rs:30:5: 30:15 (#0)): 1
    
    Deadlock #2
    State ID: s100
    Description: Deadlock state with blocked resources
    Tokens:
      incorrect_use_condvar::{closure#0}_11 (src/main.rs:19:23: 19:50 (#0)): 1
      main_0_wait (src/main.rs:5:5: 5:28 (#0)): 1
      incorrect_use_condvar_10 (src/main.rs:23:14: 23:33 (#0)): 1
```

Result Description:
Deadlock 1 indicates that the program is currently in state s105, which contains resources Condvar, incorrect_use_condvar::{closure#0}_2, main_0_wait, incorrect_use_condvar_16, corresponding to source code locations src/main.rs:11:46: 11:60 (#0), src/main.rs:14:18: 14:37 (#0), src/main.rs:5:5: 5:28 (#0), src/main.rs:30:5: 30:15 (#0) respectively.
main_0_wait represents the main function waiting for the function call to return, incorrect_use_condvar::{closure#0}_2 represents the blocked position of the closure generated by the incorrect_use_condvar function, Condvar represents the condition variable resource, and incorrect_use_condvar_16 represents the blocked position of the incorrect_use_condvar function.
The corresponding program behavior is that the main function calls the incorrect_use_condvar function, the incorrect_use_condvar function blocks waiting for the closure to return, and at this time incorrect_use_condvar holds lock mu1, causing the closure to block at line 14.

Deadlock 2 is similar, but with different blocking behavior. In deadlock 2, after the closure acquires the lock, it waits for a signal, while the incorrect_use_condvar function blocks at line 23 and cannot notify the closure.


## 3.2 Data Race Detection (using example/address_reuse as example)
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

Result Description: Data race detection mainly focuses on static data and operations on types that implement Sync in unsafe code, detecting read-write inconsistencies or write conflicts.


## 3.3 Atomicity Violation Detection (using example/atomic_se as example)
```bash
Analysis Tool: Petri Net Atomicity Violation Detector
    Analysis Time: 362.735µs
    Violation Found: true
    
    Found 3 atomicity violation patterns:
    
    Violation Pattern #1:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:16:9: 16:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
    
    Violation Pattern #2:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:16:9: 16:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:22:9: 22:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      3. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
    
    Violation Pattern #3:
    - Load Operation: AliasId { instance_id: NodeIndex(125), local: _2 } at src/main.rs:30:17: 30:43 (#0) (Relaxed)
    - Conflicting Store Operations:
      1. Store at src/main.rs:22:9: 22:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
      2. Store at src/main.rs:27:9: 27:39 (#0) (Relaxed) on AliasId { instance_id: NodeIndex(125), local: _2 }
```

Result Description: For atomicity violations, the tool mainly detects whether there are two store operations from different threads or within the same thread during load operations, which can cause undefined behavior when judging the load results.
