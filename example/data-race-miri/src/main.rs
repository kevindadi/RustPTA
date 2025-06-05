

use std::sync::atomic::*;
use std::thread::{self, spawn};

#[derive(Copy, Clone)]
struct EvilSend<T>(pub T);

unsafe impl<T> Send for EvilSend<T> {}
unsafe impl<T> Sync for EvilSend<T> {}

fn test_fence_sync() {
    static SYNC: AtomicUsize = AtomicUsize::new(0);

    let mut var = 0u32;
    let ptr = &mut var as *mut u32;
    let evil_ptr = EvilSend(ptr);

    let j1 = spawn(move || {
        let evil_ptr = evil_ptr; 
        unsafe { *evil_ptr.0 = 1 };
        fence(Ordering::Release);
        SYNC.store(1, Ordering::Relaxed)
    });

    let j2 = spawn(move || {
        let evil_ptr = evil_ptr; 
        if SYNC.load(Ordering::Relaxed) == 1 {
            fence(Ordering::Acquire);
            unsafe { *evil_ptr.0 }
        } else {
            panic!(); 
        }
    });

    j1.join().unwrap();
    j2.join().unwrap();
}

fn test_multiple_reads() {
    let mut var = 42u32;
    let ptr = &mut var as *mut u32;
    let evil_ptr = EvilSend(ptr);

    let j1 = spawn(move || unsafe { *{ evil_ptr }.0 });
    let j2 = spawn(move || unsafe { *{ evil_ptr }.0 });
    let j3 = spawn(move || unsafe { *{ evil_ptr }.0 });
    let j4 = spawn(move || unsafe { *{ evil_ptr }.0 });

    assert_eq!(j1.join().unwrap(), 42);
    assert_eq!(j2.join().unwrap(), 42);
    assert_eq!(j3.join().unwrap(), 42);
    assert_eq!(j4.join().unwrap(), 42);

    var = 10;
    assert_eq!(var, 10);
}

pub fn test_rmw_no_block() {
    static SYNC: AtomicUsize = AtomicUsize::new(0);

    let mut a = 0u32;
    let b = &mut a as *mut u32;
    let c = EvilSend(b);

    unsafe {
        let j1 = spawn(move || {
            let c = c; 
            *c.0 = 1;
            SYNC.store(1, Ordering::Release);
        });

        let j2 = spawn(move || {
            if SYNC.swap(2, Ordering::Relaxed) == 1 {
                
            }
        });

        let j3 = spawn(move || {
            let c = c; 
            if SYNC.load(Ordering::Acquire) == 2 {
                *c.0
            } else {
                0
            }
        });

        j1.join().unwrap();
        j2.join().unwrap();
        let v = j3.join().unwrap();
        assert!(v == 1 || v == 2); 
    }
}

pub fn test_simple_release() {
    static SYNC: AtomicUsize = AtomicUsize::new(0);

    let mut a = 0u32;
    let b = &mut a as *mut u32;
    let c = EvilSend(b);

    unsafe {
        let j1 = spawn(move || {
            let c = c; 
            *c.0 = 1;
            SYNC.store(1, Ordering::Release);
        });

        let j2 = spawn(move || {
            let c = c; 
            if SYNC.load(Ordering::Acquire) == 1 {
                *c.0
            } else {
                0
            }
        });

        j1.join().unwrap();
        assert_eq!(j2.join().unwrap(), 1); 
    }
}

fn test_local_variable_lazy_write() {
    static P: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

    
    
    let mut val: u8 = 0;

    let t1 = std::thread::spawn(|| {
        while P.load(Ordering::Relaxed).is_null() {
            std::hint::spin_loop();
        }
        unsafe {
            
            let ptr = P.load(Ordering::Relaxed);
            *ptr = 127;
        }
    });

    
    
    
    
    P.store(std::ptr::addr_of_mut!(val), Ordering::Relaxed);

    
    t1.join().unwrap();

    
    assert_eq!(val, 127);
}


fn test_read_read_race1() {
    let a = AtomicU16::new(0);

    thread::scope(|s| {
        s.spawn(|| {
            let ptr = &a as *const AtomicU16 as *mut u16;
            unsafe { ptr.read() };
        });
        s.spawn(|| {
            thread::yield_now();
            std::thread::sleep(std::time::Duration::from_secs(1));
            a.load(Ordering::SeqCst);
        });
    });
}


fn test_read_read_race2() {
    let a = AtomicU16::new(0);

    thread::scope(|s| {
        s.spawn(|| {
            a.load(Ordering::SeqCst);
        });
        s.spawn(|| {
            thread::yield_now();

            let ptr = &a as *const AtomicU16 as *mut u16;
            unsafe { ptr.read() };
        });
    });
}

fn mixed_size_read_read() {
    fn convert(a: &AtomicU16) -> &[AtomicU8; 2] {
        unsafe { std::mem::transmute(a) }
    }

    let a = AtomicU16::new(0);
    let a16 = &a;
    let a8 = convert(a16);

    
    thread::scope(|s| {
        s.spawn(|| {
            a16.load(Ordering::SeqCst);
        });
        s.spawn(|| {
            a8[0].load(Ordering::SeqCst);
        });
    });
}

fn failing_rmw_is_read() {
    let a = AtomicUsize::new(0);
    thread::scope(|s| {
        s.spawn(|| unsafe {
            
            let _val = *(&a as *const AtomicUsize).cast::<usize>();
        });

        s.spawn(|| {
            
            
            a.compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst)
                .unwrap_err();
        });
    });
}

pub fn main() {
    test_fence_sync();
    test_multiple_reads();
    test_rmw_no_block();
    test_simple_release();
    test_local_variable_lazy_write();
    test_read_read_race1();
    test_read_read_race2();
    mixed_size_read_read();
    failing_rmw_is_read();
}
