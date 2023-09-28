use std::mem;
use std::sync::Mutex;

fn main() {
    let data = Box::new(42);
    let ptr = Box::into_raw(data);

    // Convert the raw pointer to a Mutex pointer
    let _ = unsafe {
        let mutex_ptr1: *mut Mutex<i32> = mem::transmute(ptr);
        let mutex_ptr2 = mutex_ptr1.as_ref().unwrap();
        let mut data1 = *(mutex_ptr1.as_ref().unwrap()).lock().unwrap();
        // Lock the mutex and access the data
        let data2 = mutex_ptr2.lock().unwrap();
        data1 += 1;
    };
}
