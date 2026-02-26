use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PnConfig {
    /// 状态图探索上限. None 表示不设限. 用于防止大型项目 OOM.
    #[serde(default = "default_state_limit")]
    pub state_limit: Option<usize>,
    /// 是否仅翻译从入口可达的函数 (入口导向). 可显著减少大型项目的网规模.
    #[serde(default = "default_true")]
    pub entry_reachable: bool,
    /// 是否在状态图构建前对 Petri 网做缩减.
    #[serde(default = "default_reduce_net")]
    pub reduce_net: bool,
    /// 是否启用部分序约简 (POR), 对独立变迁减少等价交错探索. 进阶优化.
    #[serde(default)]
    pub por_enabled: bool,
    /// 是否额外翻译使用锁/原子变量/信号量/条件变量的函数及其调用者. 防止遗漏并发交错导致的 bug.
    #[serde(default = "default_true")]
    pub translate_concurrent_roots: bool,
    #[serde(default = "default_thread_spawn")]
    pub thread_spawn: Vec<String>,
    #[serde(default = "default_thread_join")]
    pub thread_join: Vec<String>,
    #[serde(default = "default_scope_spawn")]
    pub scope_spawn: Vec<String>,
    #[serde(default = "default_scope_join")]
    pub scope_join: Vec<String>,
    #[serde(default = "default_condvar_notify")]
    pub condvar_notify: Vec<String>,
    #[serde(default = "default_condvar_wait")]
    pub condvar_wait: Vec<String>,
    #[serde(default = "default_channel_send")]
    pub channel_send: Vec<String>,
    #[serde(default = "default_channel_recv")]
    pub channel_recv: Vec<String>,
    #[serde(default = "default_atomic_load")]
    pub atomic_load: Vec<String>,
    #[serde(default = "default_atomic_store")]
    pub atomic_store: Vec<String>,
    /// Unknown 别名策略: conservative (sound) 将 Unknown 视为 Possibly; optimistic 将 Unknown 视为 Unlikely
    #[serde(default = "default_alias_unknown_policy")]
    pub alias_unknown_policy: AliasUnknownPolicy,
}

/// Unknown 别名策略: 当指针分析返回 Unknown 时如何对待
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AliasUnknownPolicy {
    /// 保守策略 (sound): Unknown 视为 Possibly，添加弧，减少漏报
    #[default]
    Conservative,
    /// 乐观策略: Unknown 视为 Unlikely，不添加弧，减少误报
    Optimistic,
}

impl Default for PnConfig {
    fn default() -> Self {
        Self {
            state_limit: default_state_limit(),
            entry_reachable: true,
            reduce_net: default_reduce_net(),
            por_enabled: false,
            translate_concurrent_roots: default_true(),
            thread_spawn: default_thread_spawn(),
            thread_join: default_thread_join(),
            scope_spawn: default_scope_spawn(),
            scope_join: default_scope_join(),
            condvar_notify: default_condvar_notify(),
            condvar_wait: default_condvar_wait(),
            channel_send: default_channel_send(),
            channel_recv: default_channel_recv(),
            atomic_load: default_atomic_load(),
            atomic_store: default_atomic_store(),
            alias_unknown_policy: default_alias_unknown_policy(),
        }
    }
}

fn default_alias_unknown_policy() -> AliasUnknownPolicy {
    AliasUnknownPolicy::Conservative
}

impl PnConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        let config: PnConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;
        Ok(config)
    }
}

fn default_state_limit() -> Option<usize> {
    Some(50_000)
}

fn default_true() -> bool {
    true
}

fn default_reduce_net() -> bool {
    true
}

// Default values matching original hardcoded patterns
fn default_thread_spawn() -> Vec<String> {
    vec![
        r"std::thread[:a-zA-Z0-9_#\{\}]*::spawn".to_string(),
        r"tokio::task::spawn".to_string(),
        r"tokio::runtime::Runtime::spawn".to_string(),
        r"async_std::task::spawn".to_string(),
        r"smol::Task::spawn".to_string(),
        r"smol::spawn".to_string(),
        r"rayon::spawn".to_string(),
    ]
}

fn default_thread_join() -> Vec<String> {
    vec![
        r"std::thread[:a-zA-Z0-9_#\{\}]*::join".to_string(),
        r"std::thread::JoinHandle::try_join".to_string(),
        r"tokio::task::JoinHandle::await".to_string(),
        r"tokio::task::JoinHandle::blocking_on".to_string(),
    ]
}

fn default_scope_spawn() -> Vec<String> {
    vec![
        r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::spawn".to_string(),
        r"std::thread::scope::Scope::spawn".to_string(),
        r"crossbeam::scope::Scope::spawn".to_string(),
        r"rayon::scope::Scope::spawn".to_string(),
    ]
}

fn default_scope_join() -> Vec<String> {
    vec![
        r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::join".to_string(),
        r"std::thread::scope::Scope::join".to_string(),
        r"crossbeam::scope::Scope::join".to_string(),
        r"rayon::scope::Scope::join".to_string(),
    ]
}

fn default_condvar_notify() -> Vec<String> {
    vec![r"condvar[:a-zA-Z0-9_#\{\}]*::notify".to_string()]
}

fn default_condvar_wait() -> Vec<String> {
    vec![r"condvar[:a-zA-Z0-9_#\{\}]*::wait".to_string()]
}

fn default_channel_send() -> Vec<String> {
    vec![r"mpsc[:a-zA-Z0-9_#\{\}]*::send".to_string()]
}

fn default_channel_recv() -> Vec<String> {
    vec![r"mpsc[:a-zA-Z0-9_#\{\}]*::recv".to_string()]
}

fn default_atomic_load() -> Vec<String> {
    vec![r"atomic[:a-zA-Z0-9]*::load".to_string()]
}

fn default_atomic_store() -> Vec<String> {
    vec![r"atomic[:a-zA-Z0-9]*::store".to_string()]
}
