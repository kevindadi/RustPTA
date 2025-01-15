use rayon::prelude::*;
use std::sync::{Arc, Mutex};

fn main() {
    let lock = Arc::new(Mutex::new(0));

    let lock_clone = Arc::clone(&lock);
    let guard = lock.lock().unwrap();

    (0..10).into_par_iter().for_each(|_| {
        let _inner_guard = lock_clone.lock().unwrap();
    });
}
