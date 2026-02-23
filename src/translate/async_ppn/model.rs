//! Async-PPN 子网模型: 任务生命周期库所与变迁.
//!
//! 对每个任务 i 创建:
//! - p_ready[i], p_running[i], p_blocked[i,e], p_completed[i], p_cancelled[i]
//! 对 executor 创建:
//! - p_worker (k 个 token, k 可配置)

use crate::net::structure::{Place, PlaceType};
use crate::net::{Net, PlaceId};

use super::ids::{EventId, TaskId};

/// 任务生命周期库所集合.
#[derive(Debug, Clone)]
pub struct TaskLifecyclePlaces {
    pub task_id: TaskId,
    pub ready: PlaceId,
    pub running: PlaceId,
    /// blocked[event_id] -> PlaceId
    pub blocked: Vec<(EventId, PlaceId)>,
    pub completed: PlaceId,
    pub cancelled: Option<PlaceId>,
}

/// 异步调度器相关库所与变迁.
#[derive(Debug, Clone, Default)]
pub struct AsyncSchedulerState {
    pub worker_place: Option<PlaceId>,
    pub worker_count: u64,
    pub task_places: Vec<TaskLifecyclePlaces>,
}

impl AsyncSchedulerState {
    pub fn new(worker_count: u64) -> Self {
        Self {
            worker_place: None,
            worker_count,
            task_places: Vec::new(),
        }
    }

    /// 获取任务 i 的 p_blocked 库所(按 event 查找).
    pub fn blocked_place(&self, task_id: TaskId, event: EventId) -> Option<PlaceId> {
        self.task_places
            .get(task_id.index())
            .and_then(|tp| {
                tp.blocked
                    .iter()
                    .find(|(e, _)| *e == event)
                    .map(|(_, p)| *p)
            })
    }

    /// 获取任务 i 的 p_ready 库所.
    pub fn ready_place(&self, task_id: TaskId) -> Option<PlaceId> {
        self.task_places.get(task_id.index()).map(|tp| tp.ready)
    }

    /// 获取任务 i 的 p_running 库所.
    pub fn running_place(&self, task_id: TaskId) -> Option<PlaceId> {
        self.task_places.get(task_id.index()).map(|tp| tp.running)
    }

    /// 获取任务 i 的 p_completed 库所.
    pub fn completed_place(&self, task_id: TaskId) -> Option<PlaceId> {
        self.task_places.get(task_id.index()).map(|tp| tp.completed)
    }
}

/// 在 Net 上添加 executor 的 p_worker 库所.
pub fn add_worker_place(net: &mut Net, worker_count: u64) -> PlaceId {
    let place = Place::new(
        "async_worker",
        worker_count,
        worker_count,
        PlaceType::Resources,
        String::new(),
    );
    net.add_place(place)
}

/// 为任务 i 添加生命周期库所.
pub fn add_task_lifecycle_places(
    net: &mut Net,
    task_id: TaskId,
    blocked_events: &[EventId],
    with_cancelled: bool,
) -> TaskLifecyclePlaces {
    let idx = task_id.index();
    let ready = net.add_place(Place::new(
        format!("task_{}_ready", idx),
        0,
        1,
        PlaceType::Resources,
        String::new(),
    ));
    let running = net.add_place(Place::new(
        format!("task_{}_running", idx),
        0,
        1,
        PlaceType::Resources,
        String::new(),
    ));
    let mut blocked = Vec::with_capacity(blocked_events.len());
    for &ev in blocked_events {
        let p = net.add_place(Place::new(
            format!("task_{}_blocked_{}", idx, ev.index()),
            0,
            1,
            PlaceType::Resources,
            String::new(),
        ));
        blocked.push((ev, p));
    }
    let completed = net.add_place(Place::new(
        format!("task_{}_completed", idx),
        0,
        1,
        PlaceType::Resources,
        String::new(),
    ));
    let cancelled = if with_cancelled {
        Some(net.add_place(Place::new(
            format!("task_{}_cancelled", idx),
            0,
            1,
            PlaceType::Resources,
            String::new(),
        )))
    } else {
        None
    };

    TaskLifecyclePlaces {
        task_id,
        ready,
        running,
        blocked,
        completed,
        cancelled,
    }
}
