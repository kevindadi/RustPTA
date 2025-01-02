use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub struct SafeData {
    value: UnsafeCell<i32>, // UnsafeCell 提供内部可变性
    lock: AtomicBool,       // 原子锁，用于同步访问
}

unsafe impl Sync for SafeData {}

impl SafeData {
    pub fn new(val: i32) -> Self {
        SafeData {
            value: UnsafeCell::new(val),
            lock: AtomicBool::new(false),
        }
    }

    pub fn increment(&self) {
        // 自旋锁，确保同一时间只有一个线程访问
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {}

        unsafe {
            *self.value.get() += 1;
        }

        self.lock.store(false, Ordering::Release);
    }

    pub fn get(&self) -> i32 {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {}

        let result = unsafe { *self.value.get() };

        self.lock.store(false, Ordering::Release);

        result
    }
}

fn main() {
    let data = Arc::new(SafeData::new(0));

    let mut handles = vec![];

    for _ in 0..10 {
        let data_clone = Arc::clone(&data);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                data_clone.increment();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    println!("Final value: {}", data.get()); // 应该输出 10000
}
