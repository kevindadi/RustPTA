use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let mu1 = Arc::new(Mutex::new(1));
    let mu2 = mu1.clone();
    let g1 = mu1.lock().unwrap();

    let th1 = thread::spawn(move || {
        let mut g2 = mu2.lock().unwrap();
        *g2 = 2;
    });

    th1.join().unwrap();
}
