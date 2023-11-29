use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};
// #[derive(Debug, Clone)]
// struct Socket {
//     context: String,
// }

// impl Socket {
// fn connect(&mut self) {
//     self.context = Some("context".to_string());
// }

//     fn disconnet(&mut self) {
//         self.context = None;
//     }

//     fn use_context(&self) -> String {
//         self.context
//     }
// }

fn main() {
    // let m1 = Arc::new(Mutex::new(1i32));
    // // create a new mutex
    // let m2 = m1.clone();
    // // create a second reference to the mutex
    // let mut g1 = m1.lock().unwrap();
    // let mut g2 = m2.lock().unwrap();

    // // swap(&m1, &m2); // deadlock
    // // *g1 += 1;
    // unsafe {
    //     let a: MutexGuard<i32> = swap_guard(g1, g2);
    //     *a += 1;
    // }
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

// unsafe fn swap_guard(m1: MutexGuard<i32>, m2: MutexGuard<i32>) -> MutexGuard<'static, i32> {
//     *m1 += 1;
//     *m2 += 10;

//     let m = Mutex::new(*m1 + *m2);

//     drop(m1);
//     drop(m2);
//     //unsafe { m.lock().unwrap() }
// }
