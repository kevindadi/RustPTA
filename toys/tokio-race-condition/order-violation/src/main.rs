// use std::sync::{Arc, Mutex};
// use tokio::time::{sleep, Duration};
// #[derive(Debug, Clone)]
// struct MyStruct {
//     context: Arc<Mutex<Option<i32>>>,
// }

// impl MyStruct {
//     fn new() -> Self {
//         MyStruct {
//             context: Arc::new(Mutex::new(None)),
//         }
//     }

//     async fn connect(&self, value: i32) {
//         let mut inner = self.context.lock().unwrap();
//         *inner = Some(value);
//         println!("Connected with value: {}", value);
//     }

//     async fn usecontext(&self) {
//         std::thread::sleep(std::time::Duration::from_secs(10));
//         let inner = self.context.lock().unwrap();
//         if let Some(value) = *inner {
//             println!("Using context value: {}", value);
//         } else {
//             println!("Context is empty");
//         }
//     }

//     async fn disconnect(&self) {
//         // Drop the struct, releasing the Arc<Mutex<Option<i32>>>.
//         let mut inner = self.context.lock().unwrap();
//         if let Some(value) = *inner {
//             *inner = None;
//             println!("Context is empty");
//         }
//         println!("Disconnected");
//     }
// }

// #[tokio::main]
// async fn main() {
//     let my_struct = Arc::new(MyStruct::new());
//     let my_struct1 = my_struct.clone();

//     tokio::spawn(async move {
//         my_struct.connect(42).await;
//         my_struct.usecontext().await;
//     });

//     tokio::spawn(async move {
//         sleep(Duration::from_secs(1)).await;
//         // Disconnect by dropping the struct.
//         my_struct1.disconnect().await;
//     });

//     // Wait for the tasks to complete.
//     tokio::time::sleep(Duration::from_secs(2)).await;
// }

use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel::<String>();

    thread::spawn(move || {
        // 在这里通道被关闭
        drop(tx);
    });

    // 在尝试接收消息时，可能会发生 panic
    match rx.recv() {
        Ok(_) => {
            println!("Received: ");
        }
        _ => {}
    }
}
