//! Async-PPN 扩展: 建模 Rust async/await (Tokio-like) 协作式任务调度.
//!
//! 目标:
//! 1. 将语义交错限制在 `.await` 挂起点
//! 2. 支持异步相关 bug 检测

pub mod async_point;
pub mod ids;
pub mod labels;
pub mod meta;
pub mod model;

pub use async_point::{AsyncPoint, SourceLoc};
pub use ids::{EventId, TaskId};
pub use labels::OpKind;
pub use meta::TransitionMeta;
pub use model::{
    add_task_lifecycle_places, add_worker_place, AsyncSchedulerState, TaskLifecyclePlaces,
};
