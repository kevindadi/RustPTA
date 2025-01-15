use std::sync::Arc;
use std::thread;

fn main() {
    const NUM_THREADS: usize = 100;
    const NUM_ELEMENTS: usize = 1_000_0;

    let mut data = Arc::new(0u32);
    let data_ptr = data.clone();

    let mut temp1 = 0;
    let handle = thread::spawn(move || {
        let mut temp2 = 0;
        for _ in 0..1_000_0 {
            temp2 += 1;
        }
        unsafe {
            let raw_ptr = Arc::as_ptr(&data_ptr) as *mut u32;
            *raw_ptr = 42; // 修改主线程中的数据（不安全操作）
        }
    });

    let mut handles = vec![];
    let mut results = vec![0u64; NUM_THREADS];

    for thread_id in 0..NUM_THREADS {
        let handle = thread::spawn(move || {
            let mut sum = 0u64;
            for i in 1..=NUM_ELEMENTS {
                sum += i as u64;
            }
            sum
        });
        handles.push(handle);
    }

    for _ in 0..10000 {
        temp1 += 1;
    }
    data = 24.into();
    handle.join().unwrap();

    for (i, handle) in handles.into_iter().enumerate() {
        results[i] = handle.join().unwrap();
    }
}
