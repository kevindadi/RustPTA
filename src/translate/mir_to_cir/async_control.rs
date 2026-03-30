//! 异步 spawn/join（与 `mir_to_pn/async_control` 重复，不建网）

use super::BodyToCir;
use crate::{memory::pointsto::AliasId, net::structure::TransitionType};
use rustc_middle::mir::{BasicBlock, Operand};
use rustc_span::source_map::Spanned;

pub(super) fn handle_async_spawn<'translate, 'analysis, 'tcx, 'a>(
    b: &mut BodyToCir<'translate, 'analysis, 'tcx, 'a>,
    _callee_func_name: &str,
    args: &Box<[Spanned<Operand<'tcx>>]>,
    _target: &Option<BasicBlock>,
    bb_idx: BasicBlock,
    span: &str,
) {
    let closure_def_id = args
        .first()
        .and_then(|arg| b.resolve_closure_def_id(&arg.node));

    let task_id = b.async_ctx.alloc_task_id();
    if let Some(def_id) = closure_def_id {
        b.async_ctx.register_spawn(def_id, task_id);
    }

    b.emit_tt(
        &TransitionType::AsyncSpawn {
            task_id: task_id.index(),
        },
        bb_idx,
        span,
    );
}

pub(super) fn handle_async_join<'translate, 'analysis, 'tcx, 'a>(
    b: &mut BodyToCir<'translate, 'analysis, 'tcx, 'a>,
    _callee_func_name: &str,
    args: &Box<[Spanned<Operand<'tcx>>]>,
    _target: &Option<BasicBlock>,
    bb_idx: BasicBlock,
    span: &str,
) {
    let join_id = AliasId::from_place(
        b.instance_id,
        args.first().unwrap().node.place().unwrap().as_ref(),
    );
    let matching_callees = b.get_matching_spawn_callees(join_id);
    let task_id = matching_callees
        .first()
        .and_then(|d| b.async_ctx.get_task_for_spawn(*d))
        .map(|t| t.index())
        .unwrap_or(0);
    b.emit_tt(
        &TransitionType::AsyncJoin { task_id },
        bb_idx,
        span,
    );
}
