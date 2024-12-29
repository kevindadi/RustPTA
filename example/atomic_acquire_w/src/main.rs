use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    // 原子变量
    let x = Arc::new(AtomicUsize::new(0));
    let y = Arc::new(AtomicUsize::new(0));
    let z = Arc::new(AtomicUsize::new(0));

    // 克隆 Arc 引用以传递给线程
    let x1 = Arc::clone(&x);
    let y1 = Arc::clone(&y);
    let x2 = Arc::clone(&x);
    let y2 = Arc::clone(&y);
    let z2 = Arc::clone(&z);
    let x3 = Arc::clone(&x);
    let y3 = Arc::clone(&y);
    let z3 = Arc::clone(&z);

    // 线程 1: 写入 x，然后写入 y
    let t1 = thread::spawn(move || {
        x1.store(1, Ordering::Release); // 写入 x
        y1.store(1, Ordering::Release); // 写入 y
    });

    // 线程 2: 写入 z
    let t2 = thread::spawn(move || {
        z2.store(1, Ordering::Release); // 写入 z
    });

    // 线程 3: 读取 x, y 和 z
    let t3 = thread::spawn(move || {
        // z3.store(1, Ordering::Release); // 写入 z
        let r1 = x3.load(Ordering::Acquire); // 读取 x
        let r2 = y3.load(Ordering::Acquire); // 读取 y
        let r3 = z3.load(Ordering::Acquire); // 读取 z

        // 检测是否有原子性违背的情况
        if r1 == 1 && r2 == 1 && r3 == 0 {
            println!("Observed x=1, y=1, z=0: Potential atomicity violation!");
        } else {
            println!("Observed x={}, y={}, z={}", r1, r2, r3);
        }
    });

    // 等待所有线程完成
    t1.join().unwrap();
    t2.join().unwrap();
    t3.join().unwrap();
}
