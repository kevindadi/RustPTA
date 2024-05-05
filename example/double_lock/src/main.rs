use std::sync::{Arc, Mutex};
fn main() {
    let mu1 = Arc::new(Mutex::new(1));
    //loop {
    let g1 = mu1.lock().unwrap();
    let g2 = mu1.lock().unwrap();

    println!("unreachable!");
}
