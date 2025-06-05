// Copyright (c) 2024 <Wei Li>.
//
// This source code is licensed under the GNU license found in the
// LICENSE file in the root directory of this source tree.

//! 内存监视器使用示例

use std::thread;
use std::time::Duration;

// 假设我们的内存监视器在这个路径
// 在实际使用中，你需要根据你的项目结构调整这个路径
use RustPTA::util::mem_watcher::{get_memory_stats, MemoryWatcher, MemoryWatcherConfig};

fn main() {
    // 初始化日志
    env_logger::init();

    println!("=== 内存监视器演示 ===\n");

    // 1. 基本使用示例
    println!("1. 获取当前内存使用情况:");
    match get_memory_stats() {
        Ok(stats) => {
            println!("   物理内存: {:.2} MB", stats.physical_mb());
            println!("   虚拟内存: {:.2} MB", stats.virtual_mb());
            println!("   共享内存: {:.2} MB", stats.shared_mb());
        }
        Err(e) => println!("   获取内存统计失败: {}", e),
    }

    println!("\n2. 使用内存监视器进行监控:");

    // 2. 创建并配置内存监视器
    let config = MemoryWatcherConfig {
        interval_ms: 100,      // 每100毫秒检查一次
        history_size: 50,      // 保留50条历史记录
        verbose_logging: true, // 启用详细日志
    };

    let mut watcher = MemoryWatcher::new(config);

    // 3. 添加回调函数来监控内存变化
    watcher.add_callback(|stats| {
        if stats.physical_mb() > 50.0 {
            // 如果物理内存超过50MB，打印警告
            println!("   ⚠️  内存使用较高: {:.2} MB", stats.physical_mb());
        }
    });

    // 4. 开始监控
    if let Err(e) = watcher.start() {
        eprintln!("启动内存监视器失败: {}", e);
        return;
    }

    println!("   内存监视器已启动，开始模拟内存使用...");

    // 5. 模拟一些内存使用
    simulate_memory_usage();

    // 6. 获取监控期间的统计信息
    println!("\n3. 监控期间的统计信息:");
    let current = watcher.current_stats();
    let max = watcher.max_stats();
    let (physical_growth, virtual_growth) = watcher.memory_growth();

    println!(
        "   当前内存: 物理={:.2}MB, 虚拟={:.2}MB",
        current.physical_mb(),
        current.virtual_mb()
    );
    println!(
        "   峰值内存: 物理={:.2}MB, 虚拟={:.2}MB",
        max.physical_mb(),
        max.virtual_mb()
    );
    println!(
        "   内存增长: 物理={:.2}MB, 虚拟={:.2}MB",
        physical_growth, virtual_growth
    );

    // 7. 获取历史记录
    let history = watcher.history();
    println!("   历史记录数量: {}", history.len());

    if history.len() >= 2 {
        let first = &history[0];
        let last = &history[history.len() - 1];
        println!(
            "   监控期间内存变化: {:.2}MB -> {:.2}MB (物理内存)",
            first.physical_mb(),
            last.physical_mb()
        );
    }

    // 8. 停止监控
    println!("\n4. 停止监控...");
    watcher.stop();

    println!("\n=== 演示完成 ===");
}

/// 模拟内存使用的函数
fn simulate_memory_usage() {
    println!("   正在分配内存...");

    // 分配一些向量来模拟内存使用
    let mut vectors = Vec::new();

    for i in 0..10 {
        // 每次分配1MB的数据
        let mut data = Vec::with_capacity(1024 * 1024);
        for j in 0..1024 * 256 {
            // 256k 个 u32，约1MB
            data.push(i * 1000 + j);
        }
        vectors.push(data);

        // 暂停一下让监视器记录变化
        thread::sleep(Duration::from_millis(200));

        if i % 3 == 0 {
            println!("   已分配 {} MB", (i + 1));
        }
    }

    println!("   内存分配完成，保持2秒...");
    thread::sleep(Duration::from_secs(2));

    // 释放一半内存
    vectors.truncate(5);
    println!("   释放了一半内存，保持1秒...");
    thread::sleep(Duration::from_secs(1));

    // 最后释放所有内存
    vectors.clear();
    println!("   释放了所有分配的内存");
    thread::sleep(Duration::from_millis(500));
}
