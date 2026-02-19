//! Async-PPN 翻译算法.
//!
//! 将 tokio::spawn + JoinHandle.await 与 .await 挂起点翻译为 Async-PPN 子网.

use rustc_hir::def_id::DefId;
use rustc_middle::mir::Body;
use rustc_middle::ty::TyCtxt;

use crate::net::structure::{Transition, TransitionType};
use crate::net::{Net, PlaceId, TransitionId};

use super::async_ppn::{AsyncPoint, EventId, SourceLoc, TaskId, add_worker_place};

/// 从 MIR Body 中检测 async 挂起点 (Yield 终止符).
pub fn collect_async_points_from_mir<'tcx>(
    body: &Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
) -> Vec<AsyncPoint> {
    let fn_name = tcx.def_path_str(def_id);
    let mut points = Vec::new();
    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup || bb.is_empty_unreachable() {
            continue;
        }
        if let Some(ref term) = bb.terminator {
            if let rustc_middle::mir::TerminatorKind::Yield { .. } = &term.kind {
                let loc = SourceLoc {
                    file: None,
                    line: None,
                    fn_name: Some(fn_name.clone()),
                    bb: Some(bb_idx.index()),
                };
                points.push(AsyncPoint::new(points.len(), None, loc));
            }
        }
    }
    points
}

/// 为 async 任务构建生命周期子网并连接到已有 CFG.
///
/// 在已有 net 上:
/// 1. 添加 p_worker (若尚未添加)
/// 2. 为 task_id 添加生命周期库所
/// 3. 添加 t_spawn: [from_place] -> [to_place] + p_ready[task]
/// 4. 添加 t_poll: p_ready + p_worker -> p_running
/// 5. 为每个 await 点添加 await_ready / await_pending
/// 6. 添加 t_done: p_running -> p_completed + p_worker
/// 7. 添加 t_wake: p_blocked -> p_ready
pub struct AsyncNetBuilder<'a> {
    pub net: &'a mut Net,
    pub worker_place: PlaceId,
    pub worker_count: u64,
    pub next_task_id: usize,
    pub next_event_id: usize,
}

impl<'a> AsyncNetBuilder<'a> {
    pub fn new(net: &'a mut Net, worker_count: u64) -> Self {
        let worker_place = add_worker_place(net, worker_count);
        Self {
            net,
            worker_place,
            worker_count,
            next_task_id: 0,
            next_event_id: 0,
        }
    }

    /// 若 net 已有 worker place (通过其他方式添加), 使用此构造函数.
    pub fn with_existing_worker(
        net: &'a mut Net,
        worker_place: PlaceId,
        worker_count: u64,
    ) -> Self {
        Self {
            net,
            worker_place,
            worker_count,
            next_task_id: 0,
            next_event_id: 0,
        }
    }

    pub fn alloc_task_id(&mut self) -> TaskId {
        let id = TaskId::new(self.next_task_id);
        self.next_task_id += 1;
        id
    }

    pub fn alloc_event_id(&mut self) -> EventId {
        let id = EventId::new(self.next_event_id);
        self.next_event_id += 1;
        id
    }

    /// 添加 t_spawn: from_place -> to_place, 并产生 1 token 到 p_ready[task].
    pub fn add_spawn_transition(
        &mut self,
        task_id: TaskId,
        from_place: PlaceId,
        to_place: PlaceId,
        p_ready: PlaceId,
        name: &str,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("{}_spawn_{}", name, task_id.index()),
                TransitionType::AsyncSpawn {
                    task_id: task_id.index(),
                },
            ));
        self.net.add_input_arc(from_place, t, 1);
        self.net.add_output_arc(to_place, t, 1);
        self.net.add_output_arc(p_ready, t, 1);
        t
    }

    /// 添加 t_poll: p_ready + p_worker -> p_running.
    pub fn add_poll_transition(
        &mut self,
        task_id: TaskId,
        p_ready: PlaceId,
        p_running: PlaceId,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("poll_{}", task_id.index()),
                TransitionType::AsyncPoll {
                    task_id: task_id.index(),
                },
            ));
        self.net.add_input_arc(p_ready, t, 1);
        self.net.add_input_arc(self.worker_place, t, 1);
        self.net.add_output_arc(p_running, t, 1);
        t
    }

    /// 添加 t_await_ready: p_running + seg_from -> p_running + seg_to (不释放 worker).
    pub fn add_await_ready_transition(
        &mut self,
        task_id: TaskId,
        await_point: usize,
        p_running: PlaceId,
        seg_from: PlaceId,
        seg_to: PlaceId,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("await_ready_{}_{}", task_id.index(), await_point),
                TransitionType::AwaitReady {
                    task_id: task_id.index(),
                    await_point,
                },
            ));
        self.net.add_input_arc(p_running, t, 1);
        self.net.add_output_arc(p_running, t, 1);
        self.net.add_input_arc(seg_from, t, 1);
        self.net.add_output_arc(seg_to, t, 1);
        t
    }

    /// 添加 t_await_pending: p_running + seg_from -> p_blocked + p_worker (释放 worker).
    pub fn add_await_pending_transition(
        &mut self,
        task_id: TaskId,
        await_point: usize,
        p_running: PlaceId,
        p_blocked: PlaceId,
        seg_from: PlaceId,
        event_id: Option<EventId>,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("await_pending_{}_{}", task_id.index(), await_point),
                TransitionType::AwaitPending {
                    task_id: task_id.index(),
                    await_point,
                    event_id: event_id.map(|e| e.index()),
                },
            ));
        self.net.add_input_arc(p_running, t, 1);
        self.net.add_output_arc(p_blocked, t, 1);
        self.net.add_output_arc(self.worker_place, t, 1);
        self.net.add_input_arc(seg_from, t, 1);
        t
    }

    /// 添加 t_wake: p_blocked -> p_ready.
    pub fn add_wake_transition(
        &mut self,
        task_id: TaskId,
        event_id: EventId,
        p_blocked: PlaceId,
        p_ready: PlaceId,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("wake_{}_{}", task_id.index(), event_id.index()),
                TransitionType::AsyncWake {
                    task_id: task_id.index(),
                    event_id: event_id.index(),
                },
            ));
        self.net.add_input_arc(p_blocked, t, 1);
        self.net.add_output_arc(p_ready, t, 1);
        t
    }

    /// 添加 t_done: p_running + seg_from -> p_completed + p_worker.
    pub fn add_done_transition(
        &mut self,
        task_id: TaskId,
        p_running: PlaceId,
        p_completed: PlaceId,
        seg_from: PlaceId,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("done_{}", task_id.index()),
                TransitionType::AsyncDone {
                    task_id: task_id.index(),
                },
            ));
        self.net.add_input_arc(p_running, t, 1);
        self.net.add_output_arc(p_completed, t, 1);
        self.net.add_output_arc(self.worker_place, t, 1);
        self.net.add_input_arc(seg_from, t, 1);
        t
    }

    /// 添加 t_join: from_place + p_completed -> to_place (消费 completed token).
    pub fn add_join_transition(
        &mut self,
        task_id: TaskId,
        from_place: PlaceId,
        to_place: PlaceId,
        p_completed: PlaceId,
        name: &str,
    ) -> TransitionId {
        let t = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("{}_join_{}", name, task_id.index()),
                TransitionType::AsyncJoin {
                    task_id: task_id.index(),
                },
            ));
        self.net.add_input_arc(from_place, t, 1);
        self.net.add_input_arc(p_completed, t, 1);
        self.net.add_output_arc(to_place, t, 1);
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Net;
    use crate::net::structure::{Place, PlaceType};

    /// 构建简单的 tokio::spawn + JoinHandle.await 网,验证生命周期库所与变迁存在.
    #[test]
    fn async_spawn_join_basic() {
        let mut net = Net::empty();
        let main_start = net.add_place(Place::new(
            "main_start",
            1,
            1,
            PlaceType::FunctionStart,
            String::new(),
        ));
        let main_end = net.add_place(Place::new(
            "main_end",
            0,
            1,
            PlaceType::FunctionEnd,
            String::new(),
        ));
        let task_start = net.add_place(Place::new(
            "task_start",
            0,
            1,
            PlaceType::FunctionStart,
            String::new(),
        ));
        let task_end = net.add_place(Place::new(
            "task_end",
            0,
            1,
            PlaceType::FunctionEnd,
            String::new(),
        ));

        let tp = crate::translate::async_ppn::add_task_lifecycle_places(
            &mut net,
            TaskId::new(0),
            &[],
            false,
        );

        let mut builder = AsyncNetBuilder::new(&mut net, 1);
        let task_id = TaskId::new(0);

        let _t_spawn = builder
            .net
            .add_transition(Transition::new_with_transition_type(
                "spawn_0",
                TransitionType::AsyncSpawn { task_id: 0 },
            ));
        builder.net.add_input_arc(main_start, _t_spawn, 1);
        builder.net.add_output_arc(tp.ready, _t_spawn, 1);

        let _t_poll = builder.add_poll_transition(task_id, tp.ready, tp.running);
        builder.net.add_output_arc(task_start, _t_poll, 1);

        let _t_done = builder.add_done_transition(task_id, tp.running, tp.completed, task_end);

        let _t_join =
            builder.add_join_transition(task_id, main_start, main_end, tp.completed, "main");
        builder.net.add_input_arc(main_start, _t_join, 1);

        drop(builder);
        let place_names: Vec<_> = net.places.iter().map(|p| p.name.as_str()).collect();
        assert!(
            place_names.iter().any(|n| *n == "task_0_ready"),
            "应有 task_0_ready 库所"
        );
        assert!(
            place_names.iter().any(|n| *n == "task_0_running"),
            "应有 task_0_running 库所"
        );
        assert!(
            place_names.iter().any(|n| *n == "task_0_completed"),
            "应有 task_0_completed 库所"
        );
        assert!(
            place_names.iter().any(|n| *n == "async_worker"),
            "应有 async_worker 库所"
        );

        let trans_names: Vec<_> = net.transitions.iter().map(|t| t.name.as_str()).collect();
        assert!(
            trans_names.iter().any(|n| *n == "poll_0"),
            "应有 poll_0 变迁"
        );
        assert!(
            trans_names.iter().any(|n| n.starts_with("done_")),
            "应有 done 变迁"
        );
    }
}
