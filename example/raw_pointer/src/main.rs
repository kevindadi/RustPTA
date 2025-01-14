use std::sync::Arc;
use std::thread;

fn main() {
    let mut data = Arc::new(0u32);
    let data_ptr = data.clone();
    let handle = thread::spawn(move || {
        unsafe {
            let raw_ptr = Arc::as_ptr(&data_ptr) as *mut u32;
            *raw_ptr = 42; // 修改主线程中的数据（不安全操作）
        }
    });
    data = 24.into();
    handle.join().unwrap();
}
