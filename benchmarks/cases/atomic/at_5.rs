use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn load_then_work(v: &Arc<AtomicUsize>) -> usize {
    let base = v.load(Ordering::Relaxed);
    thread::yield_now();
    base
}

fn writer_1(v: &Arc<AtomicUsize>) { v.store(1, Ordering::Relaxed); }
fn writer_2(v: &Arc<AtomicUsize>) { writer_1(v); v.store(2, Ordering::Relaxed); }
fn writer_3(v: &Arc<AtomicUsize>) { writer_2(v); v.store(3, Ordering::Relaxed); }
fn writer_4(v: &Arc<AtomicUsize>) { writer_3(v); v.store(4, Ordering::Relaxed); }
fn writer_5(v: &Arc<AtomicUsize>) { writer_4(v); v.store(5, Ordering::Relaxed); }

fn main() {
    let v = Arc::new(AtomicUsize::new(0));
    let mut hs = Vec::new();
    {
        let r = Arc::clone(&v);
        hs.push(thread::spawn(move || { let _ = load_then_work(&r); }));
    }
    for _ in 0..6 {
        let w = Arc::clone(&v);
        hs.push(thread::spawn(move || writer_5(&w)));
    }
    for h in hs { let _ = h.join(); }
}
