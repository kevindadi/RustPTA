use std::thread;

static mut COUNTER: u64 = 0;

fn main() {
    // 创建多个线程同时访问静态可变变量

    let handle1 = thread::spawn(|| {
        for _ in 0..5000 {
            unsafe {
                COUNTER += 1; // 这里会产生数据竞争
            }
        }
    });

    let handle2 = thread::spawn(|| {
        for _ in 0..5000 {
            unsafe {
                COUNTER += 1; // 这里会产生数据竞争
            }
        }
    });

    handle1.join();
    handle2.join();
    // 由于数据竞争，结果会小于10000
    unsafe {
        println!("Final counter value: {}", COUNTER);
    }
}
