use std::sync::Mutex;

fn main() {
    let data = Mutex::new(1);
    let _g1 = data.lock().unwrap();
    let _g2 = data.lock().unwrap();
}
