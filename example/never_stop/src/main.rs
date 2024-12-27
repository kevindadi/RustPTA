use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let lock1 = Arc::new(Mutex::new(0));
    let lock2 = Arc::new(Mutex::new(0));

    let lock1_clone = Arc::clone(&lock1);
    let lock2_clone = Arc::clone(&lock2);

    let handle1 = thread::spawn(move || loop {
        let _lock1 = lock1_clone.lock().unwrap();
        println!("Thread 1: Locked lock1");

        thread::sleep(Duration::from_secs(1));

        let _lock2 = lock2_clone.lock().unwrap();
        println!("Thread 1: Locked lock2");

        thread::sleep(Duration::from_secs(1));
    });

    let handle2 = thread::spawn(move || loop {
        let _lock2 = lock2.lock().unwrap();
        println!("Thread 2: Locked lock2");

        thread::sleep(Duration::from_secs(1));

        let _lock1 = lock1.lock().unwrap();
        println!("Thread 2: Locked lock1");
        thread::sleep(Duration::from_secs(1));
    });

    let handle3 = thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
    });
    handle1.join().unwrap();
    handle2.join().unwrap();
    handle3.join().unwrap();
}
