use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

static CELL: AtomicI32 = AtomicI32::new(0);

fn reader() {
    let first = CELL.load(Ordering::Relaxed);
    let second = CELL.load(Ordering::Relaxed);
    std::hint::black_box((first, second));
}

fn writer() {
    CELL.store(1, Ordering::Relaxed);
}

fn main() {
    let handle = thread::spawn(reader);
    writer();
    handle.join().unwrap();
}
