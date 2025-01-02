use std::thread;

fn main() {
    let handles = (0..4).map(|id| {
        let id = id;
        thread::spawn(move || {
            let thread_id = id * 2;
            let mut vec = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
            vec.remove(thread_id);

            println!("id:{}", thread_id);
        })
    });

    for handle in handles {
        handle.join().unwrap();
    }
}
