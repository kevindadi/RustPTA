use rayon::iter::ParallelIterator;
use rayon::prelude::IntoParallelRefIterator;
use rayon::ThreadPoolBuilder;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::sleep;

pub const CORES: usize = 16;

#[derive(Debug)]
struct Engine {
    data: u64,
}

fn main() {
    ThreadPoolBuilder::new()
        .thread_name(|i| format!("rayon-thread-{}", i))
        .build_global()
        .unwrap();

    let engine = Arc::new(RwLock::new(Engine { data: 0 }));

    rayon::spawn({
        let engine = engine.clone();
        move || loop {
            {
                // Attempt to acquire the lock after the first read task below has acquired the read lock.
                sleep(std::time::Duration::from_millis(50));
                println!("Acquiring write lock");
                let mut lock = engine.write().unwrap();
                println!("Writing data");
                lock.data += 1;
            }
            println!("Data written");
            sleep(std::time::Duration::from_secs(3));
        }
    });

    let tasks: Vec<_> = (0..CORES / 2).collect();
    for task_number in &tasks {
        // The first task wont sleep and will acquire the read lock before the above write task attempts to acquire the write lock.
        sleep(std::time::Duration::from_millis(*task_number as u64 * 100));
        my_task(*task_number, engine.clone());
    }

    sleep(std::time::Duration::from_secs(1000));
}

fn my_task(task_number: usize, lock: Arc<RwLock<Engine>>) {
    rayon::spawn(move || {
        let thread = std::thread::current();
        println!(
            "Attempting to acquire lock task={} on thread={}",
            task_number,
            thread.name().unwrap()
        );

        let data = lock.read().unwrap();
        println!("Successfully acquired lock task={}", task_number);
        sleep(std::time::Duration::from_millis(1_000));
        let mut list = Vec::new();
        list.extend(0..CORES);
        let _: Vec<_> = list
            .par_iter()
            .map(|idx| {
                let binding = std::thread::current();
                sleep(std::time::Duration::from_secs(1));
                println!(
                    "Task={} thread={} processing it={}",
                    task_number,
                    binding.name().unwrap(),
                    idx
                );
                idx
            })
            .collect();
        let thread = std::thread::current();
        // This never prints.
        println!(
            "Processing completed task={} thread={} data={:?}",
            task_number,
            thread.name().unwrap(),
            *data
        );
    });
}
