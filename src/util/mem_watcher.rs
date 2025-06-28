//! 跨平台内存使用监控器，支持 Linux 和 macOS 系统。

use std::collections::VecDeque;
use std::io::{Error, ErrorKind, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use libc::pid_t;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;

#[cfg(target_os = "macos")]
use libc::{c_int, pid_t};
#[cfg(target_os = "macos")]
use std::mem;

/// 内存使用统计信息
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryStats {
    /// 虚拟内存大小 (字节)
    pub virtual_size: u64,
    /// 物理内存使用量 (字节)
    pub physical_size: u64,
    /// 共享内存 (字节)
    pub shared_size: u64,
    /// 时间戳
    pub timestamp: Instant,
}

impl Default for MemoryStats {
    fn default() -> Self {
        Self {
            virtual_size: 0,
            physical_size: 0,
            shared_size: 0,
            timestamp: Instant::now(),
        }
    }
}

impl MemoryStats {
    /// 以 MB 为单位返回物理内存使用量
    pub fn physical_mb(&self) -> f64 {
        self.physical_size as f64 / (1024.0 * 1024.0)
    }

    /// 以 MB 为单位返回虚拟内存使用量
    pub fn virtual_mb(&self) -> f64 {
        self.virtual_size as f64 / (1024.0 * 1024.0)
    }

    /// 以 MB 为单位返回共享内存使用量
    pub fn shared_mb(&self) -> f64 {
        self.shared_size as f64 / (1024.0 * 1024.0)
    }
}

/// 内存监视器配置
#[derive(Debug, Clone)]
pub struct MemoryWatcherConfig {
    /// 监控间隔 (毫秒)
    pub interval_ms: u64,
    /// 历史数据保留数量
    pub history_size: usize,
    /// 是否启用详细日志
    pub verbose_logging: bool,
}

impl Default for MemoryWatcherConfig {
    fn default() -> Self {
        Self {
            interval_ms: 100,
            history_size: 1000,
            verbose_logging: false,
        }
    }
}

/// 内存变化回调函数类型
pub type MemoryCallback = Arc<dyn Fn(&MemoryStats) + Send + Sync>;

/// 跨平台内存监视器
pub struct MemoryWatcher {
    config: MemoryWatcherConfig,
    initial_stats: MemoryStats,
    current_stats: Arc<Mutex<MemoryStats>>,
    max_stats: Arc<Mutex<MemoryStats>>,
    history: Arc<Mutex<VecDeque<MemoryStats>>>,
    callbacks: Arc<Mutex<Vec<MemoryCallback>>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Default for MemoryWatcher {
    fn default() -> Self {
        Self::new(MemoryWatcherConfig::default())
    }
}

impl MemoryWatcher {
    pub fn new(config: MemoryWatcherConfig) -> Self {
        let initial_stats = get_memory_stats().unwrap_or_default();
        let c_history_size = config.history_size;
        Self {
            config,
            initial_stats: initial_stats.clone(),
            current_stats: Arc::new(Mutex::new(initial_stats.clone())),
            max_stats: Arc::new(Mutex::new(initial_stats)),
            history: Arc::new(Mutex::new(VecDeque::with_capacity(c_history_size))),
            callbacks: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    /// 添加内存变化回调函数
    pub fn add_callback<F>(&mut self, callback: F)
    where
        F: Fn(&MemoryStats) + Send + Sync + 'static,
    {
        self.callbacks.lock().unwrap().push(Arc::new(callback));
    }

    /// 开始监控
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err(Error::new(ErrorKind::AlreadyExists, "内存监视器已在运行"));
        }

        self.running.store(true, Ordering::Relaxed);

        let config = self.config.clone();
        let current_stats = self.current_stats.clone();
        let max_stats = self.max_stats.clone();
        let history = self.history.clone();
        let callbacks = self.callbacks.clone();
        let running = self.running.clone();

        self.handle = Some(thread::spawn(move || {
            if config.verbose_logging {
                log::info!("内存监视器开始运行，监控间隔: {}ms", config.interval_ms);
            }

            while running.load(Ordering::Relaxed) {
                if let Ok(stats) = get_memory_stats() {
                    // 更新当前统计
                    {
                        let mut current = current_stats.lock().unwrap();
                        *current = stats.clone();
                    }

                    // 更新最大值统计
                    {
                        let mut max = max_stats.lock().unwrap();
                        if stats.physical_size > max.physical_size {
                            max.physical_size = stats.physical_size;
                        }
                        if stats.virtual_size > max.virtual_size {
                            max.virtual_size = stats.virtual_size;
                        }
                        if stats.shared_size > max.shared_size {
                            max.shared_size = stats.shared_size;
                        }
                    }

                    // 更新历史记录
                    {
                        let mut hist = history.lock().unwrap();
                        if hist.len() >= config.history_size {
                            hist.pop_front();
                        }
                        hist.push_back(stats.clone());
                    }

                    // 调用回调函数
                    {
                        let callbacks_guard = callbacks.lock().unwrap();
                        for callback in callbacks_guard.iter() {
                            callback(&stats);
                        }
                    }

                    if config.verbose_logging {
                        log::debug!(
                            "内存使用: 物理={:.2}MB, 虚拟={:.2}MB, 共享={:.2}MB",
                            stats.physical_mb(),
                            stats.virtual_mb(),
                            stats.shared_mb()
                        );
                    }
                }

                thread::sleep(Duration::from_millis(config.interval_ms));
            }

            if config.verbose_logging {
                log::info!("内存监视器停止运行");
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) {
        if !self.running.load(Ordering::Relaxed) {
            return;
        }

        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }

        // 输出统计信息
        let current = self.current_stats.lock().unwrap();
        let max = self.max_stats.lock().unwrap();

        log::info!("=== 内存使用统计 ===");
        log::info!(
            "初始内存使用: 物理={:.2}MB, 虚拟={:.2}MB",
            self.initial_stats.physical_mb(),
            self.initial_stats.virtual_mb()
        );
        log::info!(
            "最终内存使用: 物理={:.2}MB, 虚拟={:.2}MB",
            current.physical_mb(),
            current.virtual_mb()
        );
        log::info!(
            "峰值内存使用: 物理={:.2}MB, 虚拟={:.2}MB",
            max.physical_mb(),
            max.virtual_mb()
        );
        log::info!(
            "内存增长: 物理={:.2}MB, 虚拟={:.2}MB",
            current.physical_mb() - self.initial_stats.physical_mb(),
            current.virtual_mb() - self.initial_stats.virtual_mb()
        );
    }

    /// 获取当前内存统计
    pub fn current_stats(&self) -> MemoryStats {
        self.current_stats.lock().unwrap().clone()
    }

    /// 获取最大内存统计
    pub fn max_stats(&self) -> MemoryStats {
        self.max_stats.lock().unwrap().clone()
    }

    /// 获取内存使用历史
    pub fn history(&self) -> Vec<MemoryStats> {
        self.history.lock().unwrap().iter().cloned().collect()
    }

    /// 获取内存增长量 (从初始状态到当前状态)
    pub fn memory_growth(&self) -> (f64, f64) {
        let current = self.current_stats();
        (
            current.physical_mb() - self.initial_stats.physical_mb(),
            current.virtual_mb() - self.initial_stats.virtual_mb(),
        )
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for MemoryWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

// === 平台特定实现 ===

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;

    pub fn get_memory_stats_linux() -> Result<MemoryStats> {
        let mut file = File::open("/proc/self/status")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let mut vm_size = 0;
        let mut vm_rss = 0;
        let mut vm_shared = 0;

        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("VmSize:") {
                if let Ok(size) = value
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or("0")
                    .parse::<usize>()
                {
                    vm_size = size * 1024;
                }
            } else if let Some(value) = line.strip_prefix("VmRSS:") {
                if let Ok(size) = value
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or("0")
                    .parse::<usize>()
                {
                    vm_rss = size * 1024;
                }
            } else if let Some(value) = line.strip_prefix("RssFile:") {
                if let Ok(size) = value
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or("0")
                    .parse::<usize>()
                {
                    vm_shared = size * 1024;
                }
            }
        }

        Ok(MemoryStats {
            virtual_size: vm_size as u64,
            physical_size: vm_rss as u64,
            shared_size: vm_shared as u64,
            timestamp: Instant::now(),
        })
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;

    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: libc::time_value_t,
        system_time: libc::time_value_t,
        policy: c_int,
        suspend_count: c_int,
    }

    #[repr(C)]
    struct TimeValue {
        seconds: c_int,
        microseconds: c_int,
    }

    extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target_task: u32,
            flavor: c_int,
            task_info: *mut libc::c_void,
            task_info_count: *mut u32,
        ) -> c_int;
    }

    const MACH_TASK_BASIC_INFO: c_int = 20;
    const MACH_TASK_BASIC_INFO_COUNT: u32 = 10;

    pub fn get_memory_stats_macos() -> Result<MemoryStats> {
        unsafe {
            let mut info: MachTaskBasicInfo = mem::zeroed();
            let mut count = MACH_TASK_BASIC_INFO_COUNT;

            let result = task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut libc::c_void,
                &mut count,
            );

            if result != 0 {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("task_info 调用失败，错误代码: {}", result),
                ));
            }

            // 在 macOS 上，共享内存信息不容易获取，这里设为 0
            Ok(MemoryStats {
                virtual_size: info.virtual_size,
                physical_size: info.resident_size,
                shared_size: 0,
                timestamp: Instant::now(),
            })
        }
    }
}

/// 获取当前进程的内存统计信息
pub fn get_memory_stats() -> Result<MemoryStats> {
    #[cfg(target_os = "linux")]
    return linux_impl::get_memory_stats_linux();

    #[cfg(target_os = "macos")]
    return macos_impl::get_memory_stats_macos();

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(Error::new(ErrorKind::Unsupported, "不支持的操作系统"))
}

/// 获取指定进程的内存统计信息 (仅 Linux)
#[cfg(target_os = "linux")]
pub fn get_process_memory_stats(pid: pid_t) -> Result<MemoryStats> {
    let mut file = File::open(format!("/proc/{}/status", pid))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let mut vm_size = 0;
    let mut vm_rss = 0;
    let mut vm_shared = 0;

    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("VmSize:") {
            if let Ok(size) = value
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<usize>()
            {
                vm_size = size * 1024;
            }
        } else if let Some(value) = line.strip_prefix("VmRSS:") {
            if let Ok(size) = value
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<usize>()
            {
                vm_rss = size * 1024;
            }
        } else if let Some(value) = line.strip_prefix("RssFile:") {
            if let Ok(size) = value
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<usize>()
            {
                vm_shared = size * 1024;
            }
        }
    }

    Ok(MemoryStats {
        virtual_size: vm_size as u64,
        physical_size: vm_rss as u64,
        shared_size: vm_shared as u64,
        timestamp: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_stats() {
        let stats = get_memory_stats().unwrap();
        assert!(stats.physical_size > 0);
        println!("物理内存: {:.2} MB", stats.physical_mb());
        println!("虚拟内存: {:.2} MB", stats.virtual_mb());
    }

    #[test]
    fn test_memory_watcher() {
        let mut watcher = MemoryWatcher::new(MemoryWatcherConfig {
            interval_ms: 50,
            history_size: 10,
            verbose_logging: true,
        });

        watcher.add_callback(|stats| {
            println!("内存回调: {:.2} MB", stats.physical_mb());
        });

        watcher.start().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        watcher.stop();

        let history = watcher.history();
        assert!(!history.is_empty());
    }
}
