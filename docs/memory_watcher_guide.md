# 跨平台内存监视器使用指南

## 概述

这个内存监视器是一个跨平台的 Rust 库，支持 Linux 和 macOS 系统，用于监控进程的内存使用情况。它提供了丰富的功能，包括实时监控、历史记录、回调通知等。

## 主要特性

- **跨平台支持**: 支持 Linux 和 macOS 系统
- **多种内存指标**: 监控虚拟内存、物理内存和共享内存
- **实时监控**: 可配置的监控间隔
- **历史记录**: 保存内存使用历史数据
- **回调机制**: 支持自定义回调函数
- **详细统计**: 提供初始、当前、峰值和增长统计
- **线程安全**: 使用 Arc 和 Mutex 确保线程安全
- **优雅停止**: 支持 Drop trait 自动清理

## 快速开始

### 基本使用

```rust
use RustPTA::util::mem_watcher::{MemoryWatcher, MemoryWatcherConfig, get_memory_stats};

// 获取当前内存使用情况
let stats = get_memory_stats()?;
println!("物理内存: {:.2} MB", stats.physical_mb());
println!("虚拟内存: {:.2} MB", stats.virtual_mb());
```

### 创建内存监视器

```rust
// 使用默认配置
let mut watcher = MemoryWatcher::default();

// 或者使用自定义配置
let config = MemoryWatcherConfig {
    interval_ms: 100,      // 每100毫秒检查一次
    history_size: 1000,    // 保留1000条历史记录
    verbose_logging: true, // 启用详细日志
};
let mut watcher = MemoryWatcher::new(config);
```

### 启动和停止监控

```rust
// 启动监控
watcher.start()?;

// 执行你的代码...

// 停止监控（会自动输出统计信息）
watcher.stop();
```

### 添加回调函数

```rust
// 添加内存阈值监控
watcher.add_callback(|stats| {
    if stats.physical_mb() > 100.0 {
        eprintln!("警告: 内存使用超过 100MB: {:.2}MB", stats.physical_mb());
    }
});

// 添加内存泄漏检测
watcher.add_callback(|stats| {
    // 可以在这里实现更复杂的逻辑
    log::debug!("当前内存: {:.2}MB", stats.physical_mb());
});
```

## API 文档

### MemoryStats 结构

```rust
pub struct MemoryStats {
    pub virtual_size: u64,    // 虚拟内存大小 (字节)
    pub physical_size: u64,   // 物理内存使用量 (字节)
    pub shared_size: u64,     // 共享内存 (字节)
    pub timestamp: Instant,   // 时间戳
}
```

#### 便利方法

- `physical_mb()`: 以 MB 为单位返回物理内存
- `virtual_mb()`: 以 MB 为单位返回虚拟内存  
- `shared_mb()`: 以 MB 为单位返回共享内存

### MemoryWatcherConfig 配置

```rust
pub struct MemoryWatcherConfig {
    pub interval_ms: u64,        // 监控间隔 (毫秒)
    pub history_size: usize,     // 历史数据保留数量
    pub verbose_logging: bool,   // 是否启用详细日志
}
```

### MemoryWatcher 方法

- `new(config)`: 创建新的内存监视器
- `add_callback(callback)`: 添加回调函数
- `start()`: 开始监控
- `stop()`: 停止监控
- `current_stats()`: 获取当前内存统计
- `max_stats()`: 获取最大内存统计
- `history()`: 获取历史记录
- `memory_growth()`: 获取内存增长量
- `is_running()`: 检查是否正在运行

## 平台特定说明

### Linux 系统

在 Linux 系统上，内存监视器通过读取 `/proc/self/status` 文件获取内存信息：

- **VmSize**: 虚拟内存大小
- **VmRSS**: 物理内存使用量
- **RssFile**: 共享内存（近似值）

还支持监控指定进程的内存使用：

```rust
#[cfg(target_os = "linux")]
use RustPTA::util::mem_watcher::get_process_memory_stats;

let stats = get_process_memory_stats(pid)?;
```

### macOS 系统

在 macOS 系统上，内存监视器使用 Mach 系统调用：

- 使用 `task_info` 和 `MACH_TASK_BASIC_INFO` 获取内存信息
- 共享内存信息在 macOS 上不容易获取，设为 0

## 示例程序

运行示例程序来查看内存监视器的实际效果：

```bash
cargo run --example memory_watcher_demo
```

## 错误处理

内存监视器提供了完善的错误处理：

```rust
match watcher.start() {
    Ok(()) => println!("监视器启动成功"),
    Err(e) => eprintln!("启动失败: {}", e),
}
```

常见错误类型：
- `AlreadyExists`: 监视器已在运行
- `Unsupported`: 不支持的操作系统
- `Other`: 系统调用失败

## 最佳实践

1. **合理设置监控间隔**: 太短会影响性能，太长会丢失细节
2. **限制历史记录大小**: 避免内存监视器本身占用过多内存
3. **使用回调函数**: 实现实时的内存阈值监控
4. **启用详细日志**: 在调试时有助于了解内存使用模式
5. **优雅停止**: 确保调用 `stop()` 或依赖 Drop trait

## 注意事项

- 在不支持的操作系统上会返回 `Unsupported` 错误
- macOS 上的共享内存信息为 0
- 监视器在后台线程运行，确保主线程不会立即退出
- 回调函数应该尽量轻量，避免阻塞监控线程

## 故障排除

### Linux 系统

如果遇到权限问题，确保进程有读取 `/proc/self/status` 的权限。

### macOS 系统

如果遇到系统调用失败，检查是否有必要的系统权限。

### 通用问题

- 确保 `log` crate 已正确初始化
- 检查是否有足够的系统资源创建新线程
- 验证回调函数不会 panic 