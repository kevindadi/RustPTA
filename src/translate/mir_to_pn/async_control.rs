//! 异步控制：handle_async_spawn、handle_async_join

use super::BodyToPetriNet;
use crate::{
    memory::pointsto::{AliasId, ApproximateAliasKind},
    net::{Transition, TransitionId, TransitionType},
};
use rustc_middle::mir::{BasicBlock, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub(super) fn handle_async_spawn(
        &mut self,
        _callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let closure_def_id = args
            .first()
            .and_then(|arg| self.resolve_closure_def_id(&arg.node));

        let task_id = self.async_ctx.alloc_task_id();
        let worker_place = self.async_ctx.ensure_worker_place(self.net);
        let tp = self.async_ctx.add_task_simple(self.net, task_id);
        if let Some(def_id) = closure_def_id {
            self.async_ctx.register_spawn(def_id, task_id);
        }

        self.net.add_output_arc(tp.ready, bb_end, 1);

        let t_poll = self
            .net
            .add_transition(Transition::new_with_transition_type(
                format!("poll_{}", task_id.index()),
                TransitionType::AsyncPoll {
                    task_id: task_id.index(),
                },
            ));
        self.net.add_input_arc(tp.ready, t_poll, 1);
        self.net.add_input_arc(worker_place, t_poll, 1);
        self.net.add_output_arc(tp.running, t_poll, 1);

        if let Some(closure_def_id) = closure_def_id {
            if let Some((closure_start, closure_end)) =
                self.functions_map().get(&closure_def_id).copied()
            {
                self.net.add_output_arc(closure_start, t_poll, 1);
                let t_done = self
                    .net
                    .add_transition(Transition::new_with_transition_type(
                        format!("done_{}", task_id.index()),
                        TransitionType::AsyncDone {
                            task_id: task_id.index(),
                        },
                    ));
                self.net.add_input_arc(tp.running, t_done, 1);
                self.net.add_input_arc(closure_end, t_done, 1);
                self.net.add_output_arc(tp.completed, t_done, 1);
                self.net.add_output_arc(worker_place, t_done, 1);
            }
        }

        if let Some(transition) = self.net.get_transition_mut(bb_end) {
            transition.transition_type = TransitionType::AsyncSpawn {
                task_id: task_id.index(),
            };
        }
        self.connect_to_target(bb_end, target);
    }

    pub(super) fn handle_async_join(
        &mut self,
        _callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        let spawn_def_id = self
            .callgraph
            .get_spawn_calls(self.instance.def_id())
            .and_then(|spawn_calls| {
                spawn_calls.iter().find_map(|(destination, callees)| {
                    let spawn_local_id = AliasId::new(self.instance_id, *destination);
                    let alias_kind = self
                        .alias
                        .borrow_mut()
                        .alias(join_id.into(), spawn_local_id.into());
                    if matches!(
                        alias_kind,
                        ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly
                    ) {
                        callees.iter().copied().next()
                    } else {
                        None
                    }
                })
            });

        if let Some(spawn_def_id) = spawn_def_id {
            if let Some(task_id) = self.async_ctx.get_task_for_spawn(spawn_def_id) {
                if let Some(tp) = self.async_ctx.get_task_places(task_id) {
                    self.net.add_input_arc(tp.completed, bb_end, 1);
                }
            }
        }

        if let Some(transition) = self.net.get_transition_mut(bb_end) {
            transition.transition_type = TransitionType::AsyncJoin {
                task_id: spawn_def_id
                    .and_then(|d| self.async_ctx.get_task_for_spawn(d))
                    .map(|t| t.index())
                    .unwrap_or(0),
            };
        }
        self.connect_to_target(bb_end, target);
    }
}
