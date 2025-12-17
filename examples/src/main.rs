//! Tokio 异步示例，用于测试 MIR 输出功能
//! 
//! 这个示例包含多种异步操作：
//! - async/await
//! - tokio::spawn
//! - tokio::time::sleep
//! - 通道通信

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("Tokio 示例程序启动");
    
    let result = async_function(10).await;
    println!("异步函数结果: {}", result);
    
    test_spawn().await;
    
    test_channel().await;
    
    println!("程序完成");
}

async fn async_function(n: i32) -> i32 {
    sleep(Duration::from_millis(100)).await;
    n * 2
}

async fn test_spawn() {
    let handle1 = tokio::spawn(async {
        sleep(Duration::from_millis(50)).await;
        println!("任务 1 完成");
        1
    });
    
    let handle2 = tokio::spawn(async {
        sleep(Duration::from_millis(30)).await;
        println!("任务 2 完成");
        2
    });
    
    let (result1, result2) = tokio::join!(handle1, handle2);
    println!("Spawn 结果: {:?}, {:?}", result1.unwrap(), result2.unwrap());
}

async fn test_channel() {
    let (tx, mut rx) = mpsc::channel(32);
    
    let sender = tokio::spawn(async move {
        for i in 0..5 {
            tx.send(i).await.unwrap();
            sleep(Duration::from_millis(10)).await;
        }
    });
    
    let receiver = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("收到消息: {}", msg);
        }
    });
    
    tokio::join!(sender, receiver);
}

async fn test_shared_state() {
    let shared = Arc::new(tokio::sync::Mutex::new(0));
    
    let mut handles = vec![];
    for i in 0..3 {
        let shared_clone = shared.clone();
        let handle = tokio::spawn(async move {
            let mut value = shared_clone.lock().await;
            *value += i;
            println!("更新共享状态: {}", *value);
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await.unwrap();
    }
}
