//! CFG 回边检测（与 `mir_to_pn/cfg_utils` 重复，避免依赖 `mir_to_pn` 内部模块）。

use rustc_hash::FxHashSet;
use rustc_middle::mir::{BasicBlock, Body, TerminatorKind};

fn terminator_successors(body: &Body<'_>, bb: &rustc_middle::mir::BasicBlockData<'_>) -> Vec<BasicBlock> {
    let Some(term) = &bb.terminator else {
        return vec![];
    };
    let mut succs = Vec::new();
    let exclude = |t: BasicBlock| {
        let target_bb = &body.basic_blocks[t];
        target_bb.is_cleanup || target_bb.is_empty_unreachable()
    };
    match &term.kind {
        TerminatorKind::Goto { target } => {
            if !exclude(*target) {
                succs.push(*target);
            }
        }
        TerminatorKind::SwitchInt { targets, .. } => {
            for t in targets.all_targets() {
                if !exclude(*t) {
                    succs.push(*t);
                }
            }
        }
        TerminatorKind::Call { target, .. } => {
            if let Some(t) = target {
                if !exclude(*t) {
                    succs.push(*t);
                }
            }
        }
        TerminatorKind::Assert { target, .. } => {
            if !exclude(*target) {
                succs.push(*target);
            }
        }
        TerminatorKind::Drop { target, .. } => {
            if !exclude(*target) {
                succs.push(*target);
            }
        }
        TerminatorKind::FalseEdge { real_target, .. } => {
            if !exclude(*real_target) {
                succs.push(*real_target);
            }
        }
        TerminatorKind::FalseUnwind { real_target, .. } => {
            if !exclude(*real_target) {
                succs.push(*real_target);
            }
        }
        TerminatorKind::Yield { resume, .. } => {
            if !exclude(*resume) {
                succs.push(*resume);
            }
        }
        TerminatorKind::InlineAsm { targets, .. } => {
            if let Some(t) = targets.first() {
                if !exclude(*t) {
                    succs.push(*t);
                }
            }
        }
        _ => {}
    }
    succs
}

pub fn compute_back_edges(body: &Body<'_>) -> FxHashSet<(BasicBlock, BasicBlock)> {
    let mut back_edges = FxHashSet::default();
    let mut in_stack = FxHashSet::default();
    let mut visited = FxHashSet::default();

    fn dfs(
        body: &Body<'_>,
        u: BasicBlock,
        in_stack: &mut FxHashSet<BasicBlock>,
        visited: &mut FxHashSet<BasicBlock>,
        back_edges: &mut FxHashSet<(BasicBlock, BasicBlock)>,
    ) {
        let bb = &body.basic_blocks[u];
        if bb.is_cleanup || bb.is_empty_unreachable() {
            return;
        }
        if visited.contains(&u) {
            return;
        }

        in_stack.insert(u);

        for v in terminator_successors(body, bb) {
            if in_stack.contains(&v) {
                back_edges.insert((u, v));
            } else if !visited.contains(&v) {
                dfs(body, v, in_stack, visited, back_edges);
            }
        }

        in_stack.remove(&u);
        visited.insert(u);
    }

    let entry = BasicBlock::from_u32(0);
    dfs(body, entry, &mut in_stack, &mut visited, &mut back_edges);

    back_edges
}
