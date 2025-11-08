use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel();
    let mu1 = Arc::new(Mutex::new(2));

    let mu2 = mu1.clone();

    let handle = thread::spawn(move || {
        let a = mu1.lock().unwrap();
        for i in 0..5 {
            tx.send(format!("线程1的消息 {}", i)).unwrap();
        }
    });

    let data = mu2.lock().unwrap();
    let _ = rx.recv().unwrap();
    handle.join().unwrap();
}















