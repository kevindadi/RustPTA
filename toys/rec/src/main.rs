use std::sync::{Arc, Mutex};
struct X {
    x: Option<Arc<Mutex<X>>>,
}

fn main() {
    let m1 = Arc::new(Mutex::new(X { x: None }));
    let m2 = m1.clone();
    let mut g = m1.lock().unwrap();
    *g = X { x: Some(m2) };
    drop(g);
    drop(m1);
}
