//! 异步函数指针分析测试用例
//! 测试 async fn、Future、spawn

struct SharedState {
    value: i32,
}

async fn async_use_ref(x: &i32) -> i32 {
    *x
}

async fn async_with_shared() {
    let state = SharedState { value: 42 };
    let _ = async_use_ref(&state.value).await;
}

fn spawn_closure() {
    let data = vec![1i32, 2, 3];
    std::thread::spawn(move || {
        let _ = &data;
    });
}

fn main() {
    spawn_closure();
}
