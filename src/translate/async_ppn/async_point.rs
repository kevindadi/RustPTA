//! 异步挂起点抽象.
//!
//! 对应 MIR 中的 Yield/Suspend/Resume,或预计算的挂起点列表.

use super::ids::EventId;

/// 源码位置信息.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLoc {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub fn_name: Option<String>,
    pub bb: Option<usize>,
}

/// 异步挂起点,对应 `.await` 在 CFG 中的位置.
#[derive(Debug, Clone)]
pub struct AsyncPoint {
    pub id: usize,
    /// 等待的事件类型(如 Mutex(m)、Channel(ch)),若无法确定则用通用 EventId.
    pub event: Option<EventId>,
    pub loc: SourceLoc,
}

impl AsyncPoint {
    pub fn new(id: usize, event: Option<EventId>, loc: SourceLoc) -> Self {
        Self { id, event, loc }
    }

    /// 从预计算列表构建时使用,无事件信息.
    pub fn simple(id: usize, bb: usize, fn_name: impl Into<String>) -> Self {
        Self {
            id,
            event: None,
            loc: SourceLoc {
                file: None,
                line: None,
                fn_name: Some(fn_name.into()),
                bb: Some(bb),
            },
        }
    }
}
