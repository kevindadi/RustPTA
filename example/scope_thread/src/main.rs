use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

fn test_read_read_race1() {
    let a = AtomicU16::new(0);
    let data = Arc::new(Mutex::new(0));
    let data1 = data.clone();
    let lock = data.lock().unwrap();

    thread::scope(|s| {
        let th = s.spawn(|| {
            let ptr = &a as *const AtomicU16 as *mut u16;
            unsafe { ptr.read() };

            let mut lock = data1.lock().unwrap();
            *lock += 1;
        });
        s.spawn(|| {
            thread::yield_now();
            a.load(Ordering::SeqCst);
        });

        th.join().unwrap();
    });
}

fn main() {
    test_read_read_race1();
}
