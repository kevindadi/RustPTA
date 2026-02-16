//! 异步 PPN 标识符: TaskId, EventId.

use std::fmt;

use serde::{Deserialize, Serialize};

/// 任务标识符,对应 tokio::spawn 产生的每个异步任务.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskId(pub usize);

impl TaskId {
    pub fn new(idx: usize) -> Self {
        Self(idx)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task_{}", self.0)
    }
}

/// 事件标识符,对应 await 等待的事件(如 Mutex 锁、Channel 等).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventId(pub usize);

impl EventId {
    pub fn new(idx: usize) -> Self {
        Self(idx)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ev_{}", self.0)
    }
}
