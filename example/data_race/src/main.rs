use std::thread;

static mut COUNTER: u64 = 0;

fn increment() {
    unsafe {
        COUNTER += 1;
    }
}

fn main() {
    let handle1 = thread::spawn(|| {
        for _ in 0..5000 {
            increment();
        }
    });

    let handle2 = thread::spawn(|| {
        for _ in 0..5000 {
            unsafe {
                COUNTER += 1;
            }
        }
    });

    handle1.join();
    handle2.join();
    unsafe {
        println!("Final counter value: {}", COUNTER);
    }
}
