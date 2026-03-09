use std::thread;

static mut COUNTER: i32 = 0;

unsafe fn bump_n(n: i32) {
    for _ in 0..n {
        COUNTER += 1;
    }
}

unsafe fn path_1() { bump_n(10); }
unsafe fn path_2() { path_1(); bump_n(10); }
unsafe fn path_3() { path_2(); bump_n(10); }
unsafe fn path_4() { path_3(); bump_n(10); }
unsafe fn path_5() { path_4(); bump_n(10); }
unsafe fn path_6() { path_5(); bump_n(10); }
unsafe fn path_7() { path_6(); bump_n(10); }

fn main() {
    let mut hs = Vec::new();
    for _ in 0..8 {
        hs.push(thread::spawn(|| unsafe {
            path_7();
        }));
    }
    for h in hs { let _ = h.join(); }
    unsafe { let _ = COUNTER; }
}
