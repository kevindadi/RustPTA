use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self};

struct SharedData {
    counter: Mutex<usize>,
    condvar: Condvar,
}

fn main() {
    let shared_data = Arc::new(SharedData {
        counter: Mutex::new(0),
        condvar: Condvar::new(),
    });
    let thread_num = 5;
    let mut workers = Vec::new();
    let mut waits = Vec::new();

    for i in 0..thread_num {
        do_wait(i, Arc::clone(&shared_data), &mut waits);
    }
    for i in 0..thread_num {
        do_work(i, Arc::clone(&shared_data), &mut workers)
    }
    waits.into_iter().for_each(|w| w.join().unwrap());
    workers.into_iter().for_each(|w| w.join().unwrap());
}

fn do_work(i: i32, data: Arc<SharedData>, workers: &mut Vec<thread::JoinHandle<()>>) {
    workers.push(thread::spawn(move || {
        let SharedData { counter, condvar } = &*data;
        let mut data = counter.lock().unwrap();
        *data += 1;
        println!("Woker thread {} before notify: Counter {}", i, data);
        condvar.notify_one();
    }));
}
fn do_wait(i: i32, data: Arc<SharedData>, waits: &mut Vec<thread::JoinHandle<()>>) {
    waits.push(thread::spawn(move || {
        let SharedData { counter, condvar } = &*data;
        let mut data = counter.lock().unwrap();
        data = condvar.wait(data).unwrap();
        println!("   Wait thread {} after wake up: Counter {}", i, data);
    }));
}
