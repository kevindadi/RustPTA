use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    // 创建一个原子变量 x
    let x = Arc::new(AtomicUsize::new(0));

    // 克隆 Arc 引用以传递给线程
    let x1 = x.clone();
    let x2 = x.clone();
    let x3 = x.clone();

    // 线程 1：第一次写入 x
    let t1 = thread::spawn(move || {
        x1.store(1, Ordering::Relaxed); // 第一次写入
        println!("Thread 1: Stored 1 in x");
    });

    // 线程 2：第二次写入 x
    let t2 = thread::spawn(move || {
        x2.store(2, Ordering::Relaxed); // 第二次写入
        println!("Thread 2: Stored 2 in x");
    });

    // 线程 3：读取 x
    let t3 = thread::spawn(move || {
        // 读取 x 的值
        let r = x3.load(Ordering::Relaxed);
        println!("Thread 3: Loaded value from x: {}", r);

        // 模拟原子性违背检测
        if r == 1 {
            println!("Potential atomicity violation: Read x=1 after another store occurred.");
        } else if r == 2 {
            println!("Read x=2, which is the last write.");
        } else {
            println!("Unexpected value: x={}", r);
        }
    });

    // 等待所有线程完成
    t1.join().unwrap();
    t2.join().unwrap();
    t3.join().unwrap();
}
