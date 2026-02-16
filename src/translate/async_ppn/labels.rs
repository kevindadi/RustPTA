//! PPN 扩展: 变迁标签与操作类型.
//!
//! 用于区分 read/write/lock/unlock/spawn/join/await_ready/await_pending/wake/done/abort 等操作.

use serde::{Deserialize, Serialize};

/// 变迁操作类型标签,用于异步 PPN 与竞态过滤.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OpKind {
    Read,
    Write,
    Lock,
    Unlock,
    Spawn,
    Join,
    AwaitReady,
    AwaitPending,
    Wake,
    Done,
    Abort,
    /// 调度/轮询: ready -> running
    Poll,
    /// 普通控制流(非异步)
    Goto,
    Function,
    Return,
    /// 其他/未分类
    Other,
}

impl Default for OpKind {
    fn default() -> Self {
        OpKind::Other
    }
}

impl OpKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpKind::Read => "read",
            OpKind::Write => "write",
            OpKind::Lock => "lock",
            OpKind::Unlock => "unlock",
            OpKind::Spawn => "spawn",
            OpKind::Join => "join",
            OpKind::AwaitReady => "await_ready",
            OpKind::AwaitPending => "await_pending",
            OpKind::Wake => "wake",
            OpKind::Done => "done",
            OpKind::Abort => "abort",
            OpKind::Poll => "poll",
            OpKind::Goto => "goto",
            OpKind::Function => "function",
            OpKind::Return => "return",
            OpKind::Other => "other",
        }
    }
}
