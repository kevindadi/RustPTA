#[macro_use]
extern crate honggfuzz;

static mut ANSWER: i32 = 0;

fn process_data(input: &str) {
    let len = input.len();
    // 假设 input 至少有一个元素，这是不安全的假设
    unsafe {
        // 获取 input 的第一个元素的裸指针
        let mut ptr = input.as_ptr();

        // 越界访问：试图读取比实际数组长更多的元素
        for _ in 0..len + 1 {
            // 注意这里故意越界
            // 读取 ptr 指向的数据
            let data = *ptr;

            // 打印读取的数据
            println!("Data: {}", data);

            // 将指针向前移动，指向下一个元素
            ptr = ptr.add(1);
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    pub fn test_honggfuzz() {
        loop {
            fuzz!(|data: &[u8]| {
                // 这里使用 data 作为输入测试你的嵌入式 Rust 代码
                if let Ok(test_input) = std::str::from_utf8(data) {
                    process_data(test_input);
                }
            });
        }
    }
}
