use std::sync::Arc;
use std::thread;

fn std_correct() {
    use std::sync::{Condvar, Mutex};
    let mu1 = Arc::new(Mutex::new(1));
    let mu2 = mu1.clone();

    let pair1 = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = Arc::new((Mutex::new(false), Condvar::new()));

    let th1 = thread::spawn(move || {
        let _i = mu1.lock().unwrap();
        let (lock, cvar) = &*pair1;
        let mut started = lock.lock().unwrap();
        while !*started {
            started = cvar.wait(started).unwrap();
        }
    });

    let th2 = thread::spawn(move || {
        let _i = mu2.lock().unwrap();
        let (lock, cvar) = &*pair2;
        let mut started = lock.lock().unwrap();
        *started = true;
        cvar.notify_one();
    });

    th1.join().unwrap();
    th2.join().unwrap();
}

fn main() {
    std_correct();
}
