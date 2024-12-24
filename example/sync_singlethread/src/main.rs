use std::hint;
use std::sync::{atomic, Mutex};

fn main() {
    test_mutex_stdlib();
    test_rwlock_stdlib();
    test_spin_loop_hint();
    test_thread_yield_now();
}

fn test_mutex_stdlib() {
    let m = Mutex::new(0);
    {
        let _guard = m.lock().unwrap();
    }
    let a = m.lock().unwrap();
    drop(a);
    drop(m);
}

fn test_rwlock_stdlib() {
    use std::sync::RwLock;
    let rw = RwLock::new(0);
    {
        let _read_guard = rw.read().unwrap();
        let _read_guard2 = rw.read().unwrap();
        let _read_guard3 = rw.read().unwrap();
        drop(_read_guard2);
        drop(_read_guard3);
    }

    {
        let _write_guard = rw.write().unwrap();
    }
}

fn test_spin_loop_hint() {
    #[allow(deprecated)]
    atomic::spin_loop_hint();
    hint::spin_loop();
}

fn test_thread_yield_now() {
    std::thread::yield_now();
}
