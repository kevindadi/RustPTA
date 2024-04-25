use std::sync::Arc;
use std::sync::Mutex;
#[tokio::main]
async fn main() {
    let guard_A_1 = Arc::new(Mutex::new(0));
    let guard_B_1 = Arc::new(Mutex::new(1));
    let guard_A_2 = guard_A_1.clone();
    let guard_B_2 = guard_B_1.clone();
    let producer = tokio::spawn(async move {
        let mut mu1 = guard_A_1.lock().unwrap();
        *mu1 += 1;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let mut mu2 = guard_B_1.lock().unwrap();
        *mu2 += 1;
        println!("producer");
    });

    let consumer = tokio::spawn(async move {
        let mut mu2 = guard_B_2.lock().unwrap();
        *mu2 += 1;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let mut mu1 = guard_A_2.lock().unwrap();
        *mu1 += 1;
        println!("consumer");
    });

    let _ = tokio::join!(producer, consumer);
}
