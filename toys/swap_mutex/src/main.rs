use std::sync::{Arc, Mutex};

fn main() {
    let m1 = Arc::new(Mutex::new(1));
    // create a new mutex
    let m2 = m1.clone();
    // create a second reference to the mutex
    let mut g1 = m1.lock().unwrap();

    swap(&m1, &m2); // deadlock
    *g1 += 1;
}

fn swap(m1: &Mutex<i32>, m2: &Mutex<i32>) {
    // let mut g1 = m1.lock().unwrap();// acquire first mutex
    let mut g2 = m2.lock().unwrap();
    // acquire second mutex
    // let tmp = *g1;
    // // obtain the contents stored
    // *g1 = *g2;
    // // replace the contents of m1 with the contents of m2
    // *g2 = tmp;
    // replace the contents of m2 with the original contents of m1
    //drop(g1);
    drop(g2)
    // release the locks
}
