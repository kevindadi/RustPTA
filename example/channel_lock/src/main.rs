use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    // 创建通道
    let (tx, rx) = mpsc::channel();
    let shared_data = Arc::new(Mutex::new(Vec::new()));

    // 线程1：发送数据
    let tx1 = tx.clone();
    thread::spawn(move || {
        for i in 0..5 {
            tx1.send(format!("线程1的消息 {}", i)).unwrap();
        }
    });

    // 线程2：发送数据
    let tx2 = tx.clone();
    thread::spawn(move || {
        for i in 0..5 {
            tx2.send(format!("线程2的消息 {}", i)).unwrap();
        }
    });

    // 主线程接收所有消息，并分发处理
    let shared_data_clone = Arc::clone(&shared_data);
    let handle = thread::spawn(move || {
        for received in rx {
            println!("主线程接收消息: {}", received);

            // 将消息存入共享数据
            let mut data = shared_data_clone.lock().unwrap();
            data.push(received);
        }
    });

    handle.join().unwrap();

    // 打印所有消息
    let final_data = shared_data.lock().unwrap();
    println!("所有消息: {:?}", *final_data);
}
