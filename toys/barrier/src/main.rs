use std::sync::{Arc, Barrier, Mutex};

fn main() {
    let numthreads = 10;
    let my_mutex = Arc::new(Mutex::new(0));

    // We use a barrier to ensure the readout happens after all writing
    let barrier = Arc::new(Barrier::new(numthreads + 1));

    for i in 0..numthreads {
        let my_barrier = barrier.clone();
        let my_lock = my_mutex.clone();
        std::thread::spawn(move || {
            let mut guard = my_lock.lock().unwrap();
            *guard += 1;

            // Release the lock to prevent a deadlock
            drop(guard);
            println!("thread {} is ready", i);
            // Blocks the current thread until all threads have rendezvoused here.
            my_barrier.wait();
            println!("thread {} is done", i)
        });
    }

    // A barrier will block `n`-1 threads which call [`wait()`] and then wake
    // up all threads at once when the `n`th thread calls [`wait()`].
    barrier.wait();

    let answer = { *my_mutex.lock().unwrap() };
    assert_eq!(answer, numthreads);
}
