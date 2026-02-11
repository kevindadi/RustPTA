use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PnConfig {
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
}

impl Default for PnConfig {
    fn default() -> Self {
        Self {
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
        }
    }
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
