use std::mem;
use std::sync::{Arc, Mutex};

fn main() {
    let d1 = Box::new(42);
    let ptr = Box::into_raw(d1);
    let data = Arc::new(Mutex::new(Box::new(42)));
    // let mut _g1 = data.lock().unwrap();
    // Convert the raw pointer to a Mutex pointer
    let _l1 = unsafe {
        // let p1 = mem::transmute::<*const u32, *const Mutex<u32>>(ptr);
        let p1: *mut Mutex<Box<u32>> = mem::transmute(data.clone());
        println!("{:?}", *p1);
        //let l3 = Arc::new(Mutex::new((*p1).clone()));
        let _g3 = (*p1).lock().unwrap();
        // drop(p1);
        println!("unsafe");
        // let l2 = data.as_ref().lock().unwrap();
        // //let _g2 = (p1.as_ref().unwrap()).lock();
        // println!("unsafe locked");
        // let _g1 = (*p1).lock();
        // let _g2 = (*p1).lock();
        // println!("{:?}", *mutex_ptr1);
        // // let mutex_ptr2 = mutex_ptr1.as_ref().unwrap();
        // let _g1 = (*mutex_ptr1).lock().unwrap();
        // //let mut data1 = *(mutex_ptr1.as_ref().unwrap()).lock().unwrap();
        // // Lock the mutex and access the data
        // // let data2 = mutex_ptr2.lock().unwrap();
        // // data1 += 1;
        // let p2 : *mut Arc<Mutex<i32> = mem::transmute(p1);
        // let _g2 = (*(*p2)).lock().unwrap();
        // ()
    };

    // **_g1 += 1;
    // println!("{:?}", _g1);
}
