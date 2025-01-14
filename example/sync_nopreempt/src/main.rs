// We are making scheduler assumptions here.
//@compile-flags: -Zmiri-strict-provenance -Zmiri-preemption-rate=0

use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;

fn check_conditional_variables_notify_all() {
    let pair = Arc::new(((Mutex::new(())), Condvar::new()));

    let pair2 = pair.clone();
    let handle = thread::spawn(move || {
        let (lock, cvar) = &*pair2;
        let guard = lock.lock().unwrap();
        let _guard = cvar.wait(guard).unwrap();
    });

    let (_, cvar) = &*pair;
    cvar.notify_all();

    handle.join().unwrap();
}

fn check_rwlock_unlock_bug1() {
    let l = Arc::new(RwLock::new(0));

    let r1 = l.read().unwrap();
    let r2 = l.read().unwrap();

    // Make a waiting writer.
    let l2 = l.clone();
    let t = thread::spawn(move || {
        let mut w = l2.write().unwrap();
        *w += 1;
    });

    t.join().unwrap();
}

fn check_rwlock_unlock_bug2() {
    // There was a bug where when un-read-locking an rwlock by letting the last reader leaver,
    // we'd forget to wake up a writer.
    // That meant the writer thread could never run again.
    let l = Arc::new(RwLock::new(0));

    let r = l.read().unwrap();

    let l2 = l.clone();
    let h = thread::spawn(move || {
        let _w = l2.write().unwrap();
    });
    h.join().unwrap();
}

fn main() {
    check_conditional_variables_notify_all();
    check_rwlock_unlock_bug1();
    check_rwlock_unlock_bug2();
}
