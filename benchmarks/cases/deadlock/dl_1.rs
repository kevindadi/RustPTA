use std::sync::{Arc, Mutex};
use std::thread;

fn lock_ab(a: &Arc<Mutex<i32>>, b: &Arc<Mutex<i32>>) {
    let _ga = a.lock().unwrap();
    thread::yield_now();
    let _gb = b.lock().unwrap();
}

fn lock_ba(a: &Arc<Mutex<i32>>, b: &Arc<Mutex<i32>>) {
    let _gb = b.lock().unwrap();
    thread::yield_now();
    let _ga = a.lock().unwrap();
}


fn main() {
    let a = Arc::new(Mutex::new(0));
    let b = Arc::new(Mutex::new(0));
    let mut hs = Vec::new();
    {
        let a1 = Arc::clone(&a);
        let b1 = Arc::clone(&b);
        hs.push(thread::spawn(move || lock_ab(&a1, &b1)));
    }
    {
        let a2 = Arc::clone(&a);
        let b2 = Arc::clone(&b);
        hs.push(thread::spawn(move || lock_ba(&a2, &b2)));
    }
    for h in hs { let _ = h.join(); }
}
