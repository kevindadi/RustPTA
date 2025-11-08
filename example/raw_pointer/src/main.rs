use std::sync::Arc;
use std::thread;

fn main() {
    let mut data = Arc::new(0u32);
    let data_ptr = data.clone();

    let mut temp1 = 0;
    let handle = thread::spawn(move || {
        let mut temp2 = 0;
        unsafe {
            let raw_ptr = Arc::as_ptr(&data_ptr) as *mut u32;
            *raw_ptr = 42; 
        }
    });

    
    

    
    
    
    
    
    
    
    
    
    

    
    
    
    data = 24.into();
    handle.join().unwrap();

    
    
    
}
