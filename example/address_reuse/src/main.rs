


#![feature(sync_unsafe_cell)]

use std::cell::SyncUnsafeCell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;

static ADDR: AtomicUsize = AtomicUsize::new(0);
static VAL: SyncUnsafeCell<i32> = SyncUnsafeCell::new(0);

fn addr() -> usize {
    let alloc = Box::new(42);
    <*const i32>::addr(&*alloc)
}

fn thread1() {
    let alloc = addr();
    unsafe {
        VAL.get().write(24);
    }
    ADDR.store(alloc, Relaxed);
}

fn thread2() -> bool {
    
    
    for _ in 0..100 {
        let alloc = addr();
        let addr = ADDR.load(Relaxed);
        if alloc == addr {
            
            
            
            
            let val1 = unsafe { VAL.get() };

            let val2 = unsafe { val1.read() };

            assert_eq!(val2, 24);
            return true;
        }
    }

    false
}

fn main() {
    let mut success = false;
    while !success {
        let t1 = thread::spawn(thread1);
        let t2 = thread::spawn(thread2);

        t1.join().unwrap();
        
        
        
        
        
        
        
        
        success = t2.join().unwrap();

        
        ADDR.store(0, Relaxed);
        unsafe {
            VAL.get().write(0);
        }
    }
}
