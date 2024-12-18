use std::sync::{Arc, Mutex};
use std::thread;
pub fn double_lock() {
    let mu1 = Arc::new(Mutex::new(1));
    //loop {
    let g1 = mu1.lock().unwrap();
    let g2 = mu1.lock().unwrap();

    println!("unreachable!");
}

pub fn two_closures() {
    let lock_a1 = Arc::new(Mutex::new(1));
    let lock_a2 = lock_a1.clone();
    let lock_b1 = Arc::new(Mutex::new(true));
    let lock_b2 = lock_b1.clone();
    let th1 = thread::spawn(move || {
        let _b = lock_b1.lock().unwrap(); //26 31
        let _a = lock_a1.lock().unwrap();
    });
    let th2 = thread::spawn(move || {
        let _a = lock_a2.lock().unwrap();
        let _b = lock_b2.lock().unwrap(); //26 31
    });
    th1.join().unwrap();
    th2.join().unwrap();
}
