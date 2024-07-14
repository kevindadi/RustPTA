use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    let (tx1, rx1) = mpsc::channel();
    let (tx2, rx2) = mpsc::channel();
    let (tx3, rx3) = mpsc::channel();

    // Thread 1: waits to receive a message from rx3, then sends a message to tx1
    let handle1 = thread::spawn(move || {
        println!("Thread 1: Waiting to receive...");
        let received = rx3.recv().unwrap();
        println!("Thread 1: Received {}", received);
        tx1.send("Message from Thread 1").unwrap();
        println!("Thread 1: Sent message");
    });

    // Thread 2: waits to receive a message from rx1, then sends a message to tx2
    let handle2 = thread::spawn(move || {
        println!("Thread 2: Waiting to receive...");
        let received = rx1.recv().unwrap();
        println!("Thread 2: Received {}", received);
        tx2.send("Message from Thread 2").unwrap();
        println!("Thread 2: Sent message");
    });

    // Thread 3: waits to receive a message from rx2, then sends a message to tx3
    let handle3 = thread::spawn(move || {
        println!("Thread 3: Waiting to receive...");
        let received = rx2.recv().unwrap();
        println!("Thread 3: Received {}", received);
        tx3.send("Message from Thread 3").unwrap();
        println!("Thread 3: Sent message");
    });

    // Give threads some time to run
    thread::sleep(Duration::from_secs(1));

    // Uncomment one of the lines below to break the deadlock.
    // tx1.send("Initial message").unwrap();
    // tx2.send("Initial message").unwrap();
    // tx3.send("Initial message").unwrap();

    handle1.join().unwrap();
    handle2.join().unwrap();
    handle3.join().unwrap();
}
