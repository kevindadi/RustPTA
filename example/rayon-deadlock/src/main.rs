use rayon::prelude::*;
use std::sync::{Arc, Mutex};

fn main() {
    let data = Arc::new(Mutex::new(0));

    let lock = data.lock().unwrap();

    rayon::join(
        || {
            let mut guard = data.lock().unwrap();
            *guard += 1;
        },
        || {
            let mut guard = data.lock().unwrap();
            *guard += 1;
        },
    );
}
