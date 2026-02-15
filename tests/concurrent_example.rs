//! 并发指针分析测试用例
//! 测试 thread::spawn、Arc、多线程共享

use std::sync::Arc;

fn spawn_with_arc() {
    let data = Arc::new(42i32);
    let data_clone = Arc::clone(&data);
    std::thread::spawn(move || {
        let _ = *data_clone;
    });
}

fn spawn_with_vec() {
    let v = vec![1, 2, 3];
    let handle = std::thread::spawn(move || v.len());
    let _ = handle.join();
}

fn main() {
    spawn_with_arc();
    spawn_with_vec();
}
