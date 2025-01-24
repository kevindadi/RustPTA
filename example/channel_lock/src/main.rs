use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel();
    let mu1 = Arc::new(Mutex::new(2));

    let mu2 = mu1.clone();

    let handle = thread::spawn(move || {
        let a = mu1.lock().unwrap();
        for i in 0..5 {
            tx.send(format!("线程1的消息 {}", i)).unwrap();
        }
    });

    let data = mu2.lock().unwrap();
    let _ = rx.recv().unwrap();
    handle.join().unwrap();
}

// 存在一个误报，状态类生成问题
// 死锁 #1
// 状态ID: s9
// 描述: Deadlock state with blocked resources
// 标识:
//   channel_lock::main::{closure#0}_end (): 1
//   main_8 (src/main.rs:19:13: 19:22 (#0)): 1

// 死锁 #2
// 状态ID: s17
// 描述: Deadlock state with blocked resources
// 标识:
//   main_8 (src/main.rs:19:13: 19:22 (#0)): 1
//   main::{closure#0}_2 (src/main.rs:12:17: 12:36 (#0)): 1
