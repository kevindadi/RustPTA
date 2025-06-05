use std::sync::{Arc, Condvar, Mutex};
use std::thread;

fn main() {
    incorrect_use_condvar();
}

fn incorrect_use_condvar() {
    let mu1 = Arc::new(Mutex::new(1));
    let mu2 = mu1.clone();
    let pair1 = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = pair1.clone();
    let th1 = thread::spawn(move || {
        let i1 = mu1.lock().unwrap();
        let (lock, cvar) = &*pair1;
        let mut started = lock.lock().unwrap();
        while !*started {
            
            started = cvar.wait(started).unwrap();
        }
    });

    let i2 = mu2.lock().unwrap();
    let (lock, cvar) = &*pair2;

    let mut started = lock.lock().unwrap();
    *started = true;
    cvar.notify_one();

    th1.join().unwrap();
}
