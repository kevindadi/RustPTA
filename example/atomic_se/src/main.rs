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

    let t1 = thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(1));
        x1.store(1, Ordering::Relaxed); // 第一次写入
        println!("Thread 1: Stored 1 in x");
    });

    let t2 = thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        x2.store(2, Ordering::Relaxed); // 第二次写入
        println!("Thread 2: Stored 2 in x");
    });

    let t3 = thread::spawn(move || {
        x3.store(3, Ordering::Relaxed); // 第三次写入
        println!("Thread 3: Stored 3 in x");
        std::thread::sleep(std::time::Duration::from_secs(3));
        let r = x3.load(Ordering::Relaxed);
        println!("Thread 3: Loaded value from x: {}", r);

        if r == 3 {
            println!("read x is 3");
        } else {
            println!("Potential atomicity violation: Read x=1 after another store occurred.");
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
    t3.join().unwrap();
}
