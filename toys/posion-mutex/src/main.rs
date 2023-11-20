use std::sync::Mutex;

fn main() {
    let mu = Mutex::new(0);
    let _g1 = mu.lock().unwrap();
    let _g2 = mu.lock().unwrap();
}
