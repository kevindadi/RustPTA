use std::io::Error;

#[derive(Debug)]
struct Data {
    value: String,
}

// no await
// async fn fetch_data() -> Result<u32, Error> {
//     println!("fetch_data");
//     Ok(42)
// }

// async fn process_data(data: &Data) {
//     println!("process_data: {:?}", data);
// }

// #[tokio::main]
// async fn main() {
//     // let data_future = fetch_data();

//     let data = Data {
//         value: "hello".to_string(),
//     };
//     tokio::spawn(async move {
//         process_data(&data).await;
//     });
// }

use std::sync::Arc;
use tokio::sync::Mutex;

async fn update_counter(counter: Arc<Mutex<i32>>) {
    let mut num = counter.lock().await;
    *num += 1;
}

#[tokio::main]
async fn main() {
    let counter = Arc::new(Mutex::new(0));
    let handles: Vec<_> = (0..100)
        .map(|_| {
            let counter = counter.clone();
            tokio::spawn(update_counter(counter))
        })
        .collect();
    for handle in handles {
        handle.await.unwrap();
    }
    println!("Counter: {}", *counter.lock().await);
}
