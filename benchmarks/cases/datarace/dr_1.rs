use std::thread;

static mut COUNTER: i32 = 0;

unsafe fn bump_n(n: i32) {
    for _ in 0..n {
        COUNTER += 1;
    }
}

unsafe fn path_1() { bump_n(10); }

fn main() {
    let mut hs = Vec::new();
    for _ in 0..2 {
        hs.push(thread::spawn(|| unsafe {
            path_1();
        }));
    }
    for h in hs { let _ = h.join(); }
    unsafe { let _ = COUNTER; }
}
