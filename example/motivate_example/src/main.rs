use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
static mut GLOBAL_DATA: usize = 0;

fn main() {
    let atomic_var = Arc::new(AtomicUsize::new(0));
    let lock = Arc::new(Mutex::new(0));

    atomic_var.store(1, Ordering::Relaxed);

    let lock_clone = lock.clone();
    let atomic_clone = atomic_var.clone();

    let handle = thread::spawn(move || {
        let mut lock_data = lock_clone.lock().unwrap();
        *lock_data += 1;

        atomic_clone.store(2, Ordering::Relaxed);
        let mut lock_data = lock_clone.lock().unwrap();
        *lock_data += 2;
        unsafe {
            GLOBAL_DATA = 42;
        }
    });
    thread::sleep(Duration::from_secs(1));
    let atomic_value = atomic_var.load(Ordering::Relaxed);

    if atomic_value == 2 {
        println!("Atomic Violation");
    } else if atomic_value == 1 {
        println!("miri reached");
    } else {
        unsafe {
            GLOBAL_DATA = 24;
        }
    }

    handle.join().unwrap();
}
