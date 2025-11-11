use std::sync;

fn std_mutex() {
    let mu1 = sync::Mutex::new(1);
    match *mu1.lock().ok().unwrap() {
        
        1 => {}
        _ => {
            *mu1.lock().unwrap() += 1;
        } 
    };
}

fn std_rwlock() -> i32 {
    let rw1 = sync::RwLock::new(1);
    let mut a = 0;
    println!("first read ");
    match *rw1.read().unwrap() {
        1 => {
            
            a = *rw1.read().unwrap();
            println!("second read ");
        }
        _ => {
            a = *rw1.write().unwrap();
        }
    };
    a
}


fn main() {
    std_mutex();
    std_rwlock();
}
