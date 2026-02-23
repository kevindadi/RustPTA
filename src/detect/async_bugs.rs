//! 异步相关 bug 检测
//!
//! A) holding-lock-across-await: 检测 lock(m) 获取后、unlock(m) 前存在 await_pending 的路径.
//! B) cancel-safety resource leak: abort 后检查资源库所是否未回到 Init.

use crate::net::structure::TransitionType;

/// 检测是否存在 lock 跨越 await 的路径.
///
/// 完整实现需: 对每个任务,追踪 lock 获取状态,在 await_pending 时检查是否持有锁.
#[allow(dead_code)]
pub fn detect_holding_lock_across_await(
    _transition_types: &[TransitionType],
) -> Option<Vec<(usize, usize)>> {
    // 返回 None 表示未检测到
    None
}

/// 检测 cancel 后资源泄漏
///
/// 完整实现需: 在 abort 后检查资源 place 的 token 是否未归还.
#[allow(dead_code)]
pub fn detect_cancel_safety_resource_leak(
    _abort_task_id: usize,
    _resource_places: &[(usize, u64)],
) -> Option<Vec<usize>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_lock_across_await_stub_returns_none() {
        assert!(detect_holding_lock_across_await(&[]).is_none());
    }

    #[test]
    fn cancel_safety_stub_returns_none() {
        assert!(detect_cancel_safety_resource_leak(0, &[]).is_none());
    }
}
