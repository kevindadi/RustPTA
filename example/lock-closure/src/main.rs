use std::sync::{Arc, Mutex};
use std::thread;

fn one_closure_one_caller() {
    let lock_a1 = Arc::new(Mutex::new(1));
    let lock_a2 = lock_a1.clone();
    let lock_b1 = Arc::new(Mutex::new(true));
    let lock_b2 = lock_b1.clone();
    {
        let _b = lock_b1.lock().unwrap(); //10 15
        let _a = lock_a1.lock().unwrap();
    }
    let th = thread::spawn(move || {
        let _a = lock_a2.lock().unwrap();
        let _b = lock_b2.lock().unwrap(); //10 15
    });
    th.join().unwrap();
}

fn two_closures() {
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

fn main() {
    one_closure_one_caller();
    two_closures();
}
