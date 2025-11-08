use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let mut a = 0usize;
    let lock_a1 = Arc::new(Mutex::new(1));
    let lock_a2 = lock_a1.clone();
    let lock_b1 = Arc::new(Mutex::new(true));
    let lock_b2 = lock_b1.clone();
    let th1 = thread::spawn(move || {
        let _b = lock_b1.lock().unwrap(); 
        let _a = lock_a1.lock().unwrap();
    });
    let th2 = thread::spawn(move || {
        let _a = lock_a2.lock().unwrap();
        let _b = lock_b2.lock().unwrap(); 
    });

    let th3 = thread::spawn(move || {
        for i in 0..1000 {
            println!("{}", i);
        }
    });

    th1.join().unwrap();
    th2.join().unwrap();
    th3.join().unwrap();
}
