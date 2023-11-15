use std::sync;
use std::thread;

struct Foo {
    mu1: sync::Arc<sync::Mutex<i32>>,
    rw1: sync::RwLock<i32>,
}

impl Foo {
    fn new() -> Self {
        Self {
            mu1: sync::Arc::new(sync::Mutex::new(1)),
            rw1: sync::RwLock::new(1),
        }
    }

    fn std_mutex_1(&self) {
        match *self.mu1.lock().unwrap() {//   NodeIndex4 _4  36 18
            1 => {},
            _ => { self.std_rw_2(); },
        };
    }

    fn std_rw_2(&self) {
        *self.rw1.write().unwrap() += 1;//  NodeIndex3 _4   25 29
    }

    fn std_rw_1(&self) {
        match *self.rw1.read().unwrap() {//  NodeIndex5 _4   29 25
            1 => {},
            _ => { self.std_mutex_2(); },
        }
    }

    fn std_mutex_2(&self) {
        *self.mu1.lock().unwrap() += 1; //  NodeIndex6 _4 36 18
    }
}

fn main() {
    let foo = sync::Arc::new(Foo::new());
    let foo1 = foo.clone();
    let th = thread::spawn(move || {
        foo1.std_mutex_1();
    });
    foo.std_rw_1();
    // foo.std_mutex_1();
    th.join().unwrap();
}
