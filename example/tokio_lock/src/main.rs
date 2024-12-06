use std::sync::{Arc, Mutex};
use tokio;

#[tokio::main]
async fn main() {
    let data = Arc::new(Mutex::new(0));

    let tasks: Vec<_> = (0..5)
        .map(|_| {
            let data = Arc::clone(&data);
            tokio::spawn(async move {
                let mut lock = data.lock().unwrap();
                *lock += 1;
            })
        })
        .collect();

    for task in tasks {
        task.await.unwrap();
    }
}
