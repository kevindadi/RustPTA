use std::sync::{Arc, Mutex};
use std::thread;

static mut COUNTER: u64 = 0;

fn increment() {
    unsafe {
        COUNTER += 1;
    }
}

fn main() {
    let a1 = Arc::new(Mutex::new(0));
    let a2 = a1.clone();
    let handle1 = thread::spawn(move || {
        for _ in 0..5000 {
            increment();
        }
        let mut mu1 = a1.lock().unwrap();
        unsafe {
            *mu1 = COUNTER;
        }
    });

    let handle2 = thread::spawn(move || unsafe {
        loop {
            let mut mu2 = a2.lock().unwrap();
            if (*mu2 >= 4500) {
                for _ in 0..5000 {
                    COUNTER += 1;
                }
                break;
            }
        }
    });

    handle1.join();
    handle2.join();
    unsafe {
        println!("Final counter value: {}", COUNTER);
    }
}
