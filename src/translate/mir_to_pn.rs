use super::callgraph::{CallGraph, InstanceId};
use crate::{
    concurrency::{
        atomic::AtomicOrdering,
        blocking::{CondVarId, LockGuardId, LockGuardMap, LockGuardTy},
    },
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    net::{
        Idx, Net, Place, PlaceId, Transition, TransitionId, TransitionType, structure::PlaceType,
    },
    translate::callgraph::{ThreadControlKind, classify_thread_control},
    translate::structure::{FunctionRegistry, KeyApiRegex, ResourceRegistry},
    util::{format_name, has_pn_attribute},
};
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{
        BasicBlock, BasicBlockData, Const, Operand, Rvalue, Statement, StatementKind,
        SwitchTargets, TerminatorKind, UnwindAction, visit::Visitor,
    },
    ty,
};
use rustc_middle::{
    mir::{Body, Terminator},
    ty::{Instance, TyCtxt},
};
use rustc_span::source_map::Spanned;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

#[derive(Default)]
struct BasicBlockGraph {
    start_places: HashMap<BasicBlock, PlaceId>, // 每个基本块的起始库所,标识进入此块的指令指针
    sequences: HashMap<BasicBlock, Vec<PlaceId>>, // 每个基本块的指令序列,标识此块内的指令顺序
}

impl BasicBlockGraph {
    fn new() -> Self {
        Self::default()
    }

    fn register(&mut self, bb: BasicBlock, start: PlaceId) {
        self.start_places.insert(bb, start);
        self.sequences.insert(bb, vec![start]);
    }

    fn push(&mut self, bb: BasicBlock, place: PlaceId) {
        self.sequences.entry(bb).or_default().push(place);
    }

    fn start(&self, bb: BasicBlock) -> PlaceId {
        *self
            .start_places
            .get(&bb)
            .expect("basic block start place should exist")
    }

    fn last(&self, bb: BasicBlock) -> PlaceId {
        *self
            .sequences
            .get(&bb)
            .and_then(|nodes| nodes.last())
            .expect("basic block last node should exist")
    }
}

#[cfg(feature = "atomic-violation")]
#[derive(Default)]
struct SegState {
    seg_index: HashMap<usize, usize>,
    seg_place_of: HashMap<(usize, usize), PlaceId>,
    seqcst_place: Option<PlaceId>,
}

#[cfg(feature = "atomic-violation")]
impl SegState {
    fn current_seg(&self, tid: usize) -> usize {
        *self.seg_index.get(&tid).unwrap_or(&0)
    }

    fn bump(&mut self, tid: usize) -> usize {
        let next = self.current_seg(tid) + 1;
        self.seg_index.insert(tid, next);
        next
    }
}

pub struct BodyToPetriNet<'translate, 'analysis, 'tcx> {
    instance_id: InstanceId,
    instance: &'translate Instance<'tcx>,
    body: &'translate Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    callgraph: &'translate CallGraph<'tcx>,
    pub net: &'translate mut Net,
    alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
    pub lockguards: LockGuardMap<'tcx>,
    functions: &'translate FunctionRegistry,
    resources: &'translate ResourceRegistry,
    bb_graph: BasicBlockGraph,
    pub exclude_bb: HashSet<usize>,
    return_transition: TransitionId,
    entry_exit: (PlaceId, PlaceId),
    key_api_regex: &'translate KeyApiRegex,
    #[cfg(feature = "atomic-violation")]
    seg: SegState,
}

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    fn functions_map(&self) -> &HashMap<DefId, (PlaceId, PlaceId)> {
        self.functions.counter()
    }

    fn find_atomic_match(&mut self, current_id: &AliasId) -> Option<(AliasId, PlaceId)> {
        for (alias_id, place_id) in self.resources.atomic_places().iter() {
            let alias_kind = self.alias.borrow_mut().alias_atomic(*current_id, *alias_id);
            if matches!(
                alias_kind,
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly
            ) {
                return Some((*alias_id, *place_id));
            }
        }
        None
    }

    #[cfg(feature = "atomic-violation")]
    fn handle_atomic_basic_op<F>(
        &mut self,
        op_name: &str,
        current_id: AliasId,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
        _transition_builder: F,
    ) -> bool
    where
        F: FnMut(&AliasId, &AtomicOrdering, String) -> TransitionType,
    {
        self.link_atomic_operation(op_name, current_id, bb_end, target, bb_idx, span)
    }

    #[cfg(not(feature = "atomic-violation"))]
    fn handle_atomic_basic_op<F>(
        &mut self,
        op_name: &str,
        current_id: AliasId,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
        mut transition_builder: F,
    ) -> bool
    where
        F: FnMut(&AliasId, &AtomicOrdering, String) -> TransitionType,
    {
        if let Some((alias_id, resource_place)) = self.find_atomic_match(&current_id) {
            let span_owned = span.to_string();
            let intermediate_name = format!(
                "atomic_{}_in_{:?}_{:?}",
                op_name,
                current_id.instance_id.index(),
                bb_idx.index()
            );
            let intermediate_place = Place::new(
                intermediate_name,
                0,
                1,
                PlaceType::BasicBlock,
                span_owned.clone(),
            );
            let intermediate_id = self.net.add_place(intermediate_place);
            self.net.add_input_arc(intermediate_id, bb_end, 1);

            if let Some(order) = self.resources.atomic_orders().get(&current_id) {
                let transition_name = format!(
                    "atomic_{:?}_{}_{:?}_{:?}",
                    self.instance_id.index(),
                    op_name,
                    order,
                    bb_idx.index()
                );
                let transition_type = transition_builder(&alias_id, order, span_owned.clone());
                let transition =
                    Transition::new_with_transition_type(transition_name, transition_type);
                let transition_id = self.net.add_transition(transition);

                self.net.add_output_arc(intermediate_id, transition_id, 1);
                self.net.add_input_arc(resource_place, transition_id, 1);
                self.net.add_output_arc(resource_place, transition_id, 1);

                if let Some(t) = target {
                    self.net
                        .add_input_arc(self.bb_graph.start(*t), transition_id, 1);
                }
            }
            return true;
        }
        false
    }

    #[cfg(feature = "atomic-violation")]
    fn link_atomic_operation(
        &mut self,
        op_name: &str,
        current_id: AliasId,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let Some((alias_id, resource_place)) = self.find_atomic_match(&current_id) else {
            log::warn!("no alias found for atomic operation in {:?}", span);
            return false;
        };

        let Some(order) = self.resources.atomic_orders().get(&current_id).copied() else {
            log::warn!(
                "[atomic-violation] missing ordering for {} @ {:?}",
                op_name,
                span
            );
            self.connect_to_target(bb_end, target);
            return true;
        };

        let tid = self.instance_id.index();
        let span_owned = span.to_string();

        let transition_name = {
            let name = format!(
                "atomic_{:?}_{}_ord={:?}_bb={}",
                self.instance_id.index(),
                op_name,
                order,
                bb_idx.index()
            );
            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.name = name.clone();
                transition.transition_type = match op_name {
                    "load" => TransitionType::AtomicLoad(alias_id, order, span_owned.clone(), tid),
                    "store" => {
                        TransitionType::AtomicStore(alias_id, order, span_owned.clone(), tid)
                    }
                    _ => transition.transition_type.clone(),
                };
            } else {
                return false;
            }
            name
        };

        self.net.add_input_arc(resource_place, bb_end, 1);
        self.net.add_output_arc(resource_place, bb_end, 1);
        self.wire_segment_for_ordering(bb_end, tid, order);
        self.connect_to_target(bb_end, target);

        log::debug!(
            "[atomic-violation] wired {} at {:?} with ord={:?}, tid={}, alias={:?}",
            transition_name,
            span,
            order,
            tid,
            alias_id
        );

        true
    }

    #[cfg(feature = "atomic-violation")]
    fn ensure_seg_place(&mut self, tid: usize, seg: usize) -> PlaceId {
        if let Some(&place_id) = self.seg.seg_place_of.get(&(tid, seg)) {
            return place_id;
        }

        let name = format!("seg_t{}_s{}", tid, seg);
        let tokens = if seg == 0 { 1 } else { 0 };
        let place = Place::new(name, tokens, u64::MAX, PlaceType::BasicBlock, String::new());
        let place_id = self.net.add_place(place);
        self.seg.seg_place_of.insert((tid, seg), place_id);
        place_id
    }

    #[cfg(feature = "atomic-violation")]
    fn ensure_seqcst_place(&mut self) -> PlaceId {
        if let Some(place_id) = self.seg.seqcst_place {
            return place_id;
        }

        let place = Place::new(
            "SeqCst_Global",
            1,
            u64::MAX,
            PlaceType::Resources,
            String::new(),
        );
        let place_id = self.net.add_place(place);
        self.seg.seqcst_place = Some(place_id);
        place_id
    }

    #[cfg(feature = "atomic-violation")]
    fn wire_segment_for_ordering(&mut self, bb_end: TransitionId, tid: usize, ord: AtomicOrdering) {
        let current_seg = self.seg.current_seg(tid);
        let current_place = self.ensure_seg_place(tid, current_seg);

        match ord {
            AtomicOrdering::Relaxed => {
                self.net.add_input_arc(current_place, bb_end, 1);
                self.net.add_output_arc(current_place, bb_end, 1);
            }
            AtomicOrdering::Acquire | AtomicOrdering::Release | AtomicOrdering::AcqRel => {
                let next_seg = self.seg.bump(tid);
                let next_place = self.ensure_seg_place(tid, next_seg);
                self.net.add_input_arc(current_place, bb_end, 1);
                self.net.add_output_arc(next_place, bb_end, 1);
            }
            AtomicOrdering::SeqCst => {
                let next_seg = self.seg.bump(tid);
                let next_place = self.ensure_seg_place(tid, next_seg);
                self.net.add_input_arc(current_place, bb_end, 1);
                self.net.add_output_arc(next_place, bb_end, 1);
                let seqcst_place = self.ensure_seqcst_place();
                self.net.add_input_arc(seqcst_place, bb_end, 1);
                self.net.add_output_arc(seqcst_place, bb_end, 1);
            }
        }
    }

    fn find_channel_place(&mut self, channel_alias: AliasId) -> Option<PlaceId> {
        for (alias_id, node) in self.resources.channel_places().iter() {
            let alias_kind = self
                .alias
                .borrow_mut()
                .alias_atomic(channel_alias, *alias_id);
            if matches!(
                alias_kind,
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly
            ) {
                return Some(*node);
            }
        }
        None
    }

    pub fn new(
        instance_id: InstanceId,
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        callgraph: &'translate CallGraph<'tcx>,
        net: &'translate mut Net,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: LockGuardMap<'tcx>,
        functions: &'translate FunctionRegistry,
        resources: &'translate ResourceRegistry,
        entry_exit: (PlaceId, PlaceId),
        key_api_regex: &'translate KeyApiRegex,
    ) -> Self {
        #[allow(unused_mut)]
        let mut s = Self {
            instance_id,
            instance,
            body,
            tcx,
            callgraph,
            net,
            alias,
            lockguards,
            functions,
            resources,
            bb_graph: BasicBlockGraph::new(),
            exclude_bb: HashSet::new(),
            return_transition: TransitionId::new(0),
            entry_exit,
            key_api_regex,
            #[cfg(feature = "atomic-violation")]
            seg: SegState::default(),
        };

        #[cfg(feature = "atomic-violation")]
        {
            let tid = s.instance_id.index();
            s.seg.seg_index.insert(tid, 0);
            let seg_place = s.ensure_seg_place(tid, 0);
            if let Some(place) = s.net.get_place_mut(seg_place) {
                place.tokens = 1;
                if place.capacity < 1 {
                    place.capacity = 1;
                }
            }
        }

        s
    }

    pub fn translate(&mut self) {
        self.visit_body(self.body);
    }

    fn init_basic_block(&mut self, body: &Body<'tcx>, body_name: &str) {
        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                self.exclude_bb.insert(bb_idx.index());
                continue;
            }
            let bb_span = bb.terminator.as_ref().map_or("".to_string(), |term| {
                format!("{:?}", term.source_info.span)
            });

            let bb_name = format!("{}_{}", body_name, bb_idx.index());
            let bb_start_place = Place::new(bb_name, 0, 1, PlaceType::BasicBlock, bb_span);
            let bb_start = self.net.add_place(bb_start_place);
            self.bb_graph.register(bb_idx, bb_start);
        }
    }

    fn handle_start_block(&mut self, name: &str, bb_idx: BasicBlock, def_id: DefId) {
        let bb_start_name = format!("{}_{}_start", name, bb_idx.index());
        let bb_start_transition = Transition::new_with_transition_type(
            bb_start_name,
            TransitionType::Start(self.instance_id.index()),
        );
        let bb_start = self.net.add_transition(bb_start_transition);

        if let Some((func_start, _)) = self.functions_map().get(&def_id).copied() {
            self.net.add_input_arc(func_start, bb_start, 1);
        }
        self.net
            .add_output_arc(self.bb_graph.start(bb_idx), bb_start, 1);
    }

    fn handle_assert(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "assert");
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Assert);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), bb_end, 1);
        self.net
            .add_output_arc(self.bb_graph.start(*target), bb_end, 1);
    }

    fn handle_terminator(
        &mut self,
        term: &Terminator<'tcx>,
        bb_idx: BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        match &term.kind {
            TerminatorKind::Goto { target } => self.handle_goto(bb_idx, target, name),
            TerminatorKind::SwitchInt { targets, .. } => self.handle_switch(bb_idx, targets, name),
            TerminatorKind::Return => self.handle_return(bb_idx, name),
            TerminatorKind::Assert { target, .. } => {
                self.handle_assert(bb_idx, target, name);
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                unwind,
                ..
            } => {
                self.handle_call(
                    bb_idx,
                    func,
                    args,
                    destination,
                    target,
                    name,
                    &format!("{:?}", term.source_info.span),
                    unwind,
                );
            }
            TerminatorKind::Drop { place, target, .. } => {
                self.handle_drop(&bb_idx, place, target, name, bb)
            }
            // FalseEdge: 用于 match 语句,imaginary_target 是"假"边,real_target 是真正的后继
            TerminatorKind::FalseEdge { real_target, .. } => {
                self.handle_fallthrough(bb_idx, real_target, name, "false_edge");
            }
            // FalseUnwind: 用于循环,real_target 是循环体,unwind 用于 panic
            TerminatorKind::FalseUnwind { real_target, .. } => {
                self.handle_fallthrough(bb_idx, real_target, name, "false_unwind");
            }
            // Yield: generator/async 中的 yield 点
            TerminatorKind::Yield { resume, .. } => {
                self.handle_fallthrough(bb_idx, resume, name, "yield");
            }
            // InlineAsm: 内联汇编,有可选的后继块
            TerminatorKind::InlineAsm {
                targets, unwind: _, ..
            } => {
                if let Some(target) = targets.first() {
                    self.handle_fallthrough(bb_idx, target, name, "inline_asm");
                } else {
                    // 无后继块的内联汇编,连接到函数出口
                    self.handle_terminal_block(bb_idx, name, "inline_asm_noreturn");
                }
            }
            // Unreachable: 不可达代码,连接到函数出口(作为终止状态)
            TerminatorKind::Unreachable => {
                self.handle_terminal_block(bb_idx, name, "unreachable");
            }
            // UnwindResume: panic 展开恢复,连接到函数出口
            TerminatorKind::UnwindResume => {
                self.handle_terminal_block(bb_idx, name, "unwind_resume");
            }
            // UnwindTerminate: panic 展开终止(abort)
            TerminatorKind::UnwindTerminate(_) => {
                self.handle_terminal_block(bb_idx, name, "unwind_terminate");
            }
            // CoroutineDrop: 协程 drop
            TerminatorKind::CoroutineDrop => {
                self.handle_terminal_block(bb_idx, name, "coroutine_drop");
            }
            // TailCall: 尾调用优化(较新的 Rust 版本)
            TerminatorKind::TailCall { .. } => {
                // TailCall 不返回,视为函数出口
                self.handle_terminal_block(bb_idx, name, "tail_call");
            }
        }
    }

    /// 处理具有明确后继块的终止符(fallthrough 语义)
    fn handle_fallthrough(
        &mut self,
        bb_idx: BasicBlock,
        target: &BasicBlock,
        name: &str,
        kind: &str,
    ) {
        if self.exclude_bb.contains(&target.index()) {
            log::debug!(
                "Fallthrough {} from bb{} to excluded bb{}",
                kind,
                bb_idx.index(),
                target.index()
            );
            return;
        }

        let transition_name = format!("{}_{}_{}", name, bb_idx.index(), kind);
        let transition =
            Transition::new_with_transition_type(transition_name, TransitionType::Goto);
        let t_id = self.net.add_transition(transition);

        self.net.add_input_arc(self.bb_graph.last(bb_idx), t_id, 1);
        self.net
            .add_output_arc(self.bb_graph.start(*target), t_id, 1);
    }

    /// 处理没有后继块的终止符(终止状态)
    fn handle_terminal_block(&mut self, bb_idx: BasicBlock, name: &str, kind: &str) {
        let transition_name = format!("{}_{}_{}", name, bb_idx.index(), kind);
        let transition = Transition::new_with_transition_type(
            transition_name,
            TransitionType::Return(self.instance_id.index()),
        );
        let t_id = self.net.add_transition(transition);

        self.net.add_input_arc(self.bb_graph.last(bb_idx), t_id, 1);
        self.net.add_output_arc(self.entry_exit.1, t_id, 1);
    }

    fn handle_goto(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        if self.body.basic_blocks[*target].is_cleanup {
            self.handle_panic(bb_idx, name);
            return;
        }

        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "goto");
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Goto);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), bb_end, 1);

        let target_bb_start = self.bb_graph.start(*target);
        self.net.add_output_arc(target_bb_start, bb_end, 1);
    }

    fn handle_switch(&mut self, bb_idx: BasicBlock, targets: &SwitchTargets, name: &str) {
        let mut t_num = 1u8;
        for t in targets.all_targets() {
            if self.exclude_bb.contains(&t.index()) {
                continue;
            }
            let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "switch")
                + "switch"
                + t_num.to_string().as_str();
            t_num += 1;
            let bb_term_transition =
                Transition::new_with_transition_type(bb_term_name, TransitionType::Switch);
            let bb_end = self.net.add_transition(bb_term_transition);

            self.net
                .add_input_arc(self.bb_graph.last(bb_idx), bb_end, 1);
            let target_bb_start = self.bb_graph.start(*t);
            self.net.add_output_arc(target_bb_start, bb_end, 1);
        }
    }

    fn handle_return(&mut self, bb_idx: BasicBlock, name: &str) {
        let return_node = self
            .functions_map()
            .get(&self.instance.def_id())
            .map(|(_, end)| *end)
            .expect("return place missing");

        if self.return_transition.raw() == 0 {
            let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_term_transition = Transition::new_with_transition_type(
                bb_term_name,
                TransitionType::Return(self.instance_id.index()),
            );
            let bb_end = self.net.add_transition(bb_term_transition);

            self.return_transition = bb_end.clone();
        }

        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), self.return_transition, 1);
        self.net
            .add_output_arc(return_node, self.return_transition, 1);
    }

    fn create_call_transition(&mut self, bb_idx: BasicBlock, bb_term_name: &str) -> TransitionId {
        let bb_term_transition = Transition::new_with_transition_type(
            bb_term_name.to_string(),
            TransitionType::Function,
        );
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), bb_end, 1);
        bb_end
    }

    fn handle_lock_call(
        &mut self,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) -> Option<TransitionType> {
        if cfg!(feature = "atomic-violation") {
            return None;
        }

        let lockguard_id = LockGuardId::new(self.instance_id, destination.local);
        if let Some(guard) = self.lockguards.get_mut(&lockguard_id) {
            let lock_alias = lockguard_id.get_alias_id();
            let lock_node = self.resources.locks().get(&lock_alias).unwrap();

            let call_type = match &guard.lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => TransitionType::Lock(lock_node.index()),
                LockGuardTy::StdRwLockRead(_)
                | LockGuardTy::ParkingLotRead(_)
                | LockGuardTy::SpinRead(_) => TransitionType::RwLockRead(lock_node.index()),
                _ => TransitionType::RwLockWrite(lock_node.index()),
            };

            self.update_lock_transition(bb_end, lock_node);
            self.connect_to_target(bb_end, target);
            Some(call_type)
        } else {
            None
        }
    }

    fn update_lock_transition(&mut self, bb_end: TransitionId, lock_node: &PlaceId) {
        self.net.add_input_arc(*lock_node, bb_end, 1);
    }

    fn connect_to_target(&mut self, bb_end: TransitionId, target: &Option<BasicBlock>) {
        if let Some(target_bb) = target {
            self.net
                .add_output_arc(self.bb_graph.start(*target_bb), bb_end, 1);
        }
    }

    fn handle_thread_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if let Some(kind) = classify_thread_control(
            self.tcx,
            callee_def_id,
            callee_func_name,
            self.key_api_regex,
        ) {
            match kind {
                ThreadControlKind::Spawn => {
                    self.handle_spawn(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::ScopeSpawn => {
                    self.handle_scope_spawn(callee_func_name, bb_idx, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::Join => {
                    self.handle_join(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::ScopeJoin => {
                    self.handle_scope_join(callee_func_name, args, target, bb_end);
                    return true;
                }
                ThreadControlKind::RayonJoin => {
                    self.handle_rayon_join(callee_func_name, bb_idx, args, target, bb_end, span);
                    return true;
                }
            }
        }
        false
    }

    fn handle_scope_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let spawn_def_id = spawn_calls
                .iter()
                .find_map(|(destination, callees)| {
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
                .or_else(|| {
                    log::error!(
                        "No matching spawn call found for join in {:?}",
                        self.instance.def_id()
                    );
                    None
                });

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            if let Some(spawn_def_id) = spawn_def_id {
                if let Some((_, spawn_end)) = self.functions_map().get(&spawn_def_id).copied() {
                    self.net.add_input_arc(spawn_end, bb_end, 1);
                }
            }
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_scope_spawn(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        if self.return_transition.index() == 0 {
            let bb_term_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "return");
            let bb_term_transition =
                Transition::new_with_transition_type(bb_term_name, TransitionType::Function);
            self.return_transition = self.net.add_transition(bb_term_transition);
        }

        if let Some(closure_arg) = args.get(1) {
            match &closure_arg.node {
                Operand::Move(place) | Operand::Copy(place) => {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                        place_ty.kind()
                    {
                        if let Some((closure_start, closure_end)) =
                            self.functions_map().get(&closure_def_id).copied()
                        {
                            self.net.add_output_arc(closure_start, bb_end, 1);
                            self.net
                                .add_input_arc(closure_end, self.return_transition, 1);
                        }
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            if let Some((closure_start, closure_end)) =
                                self.functions_map().get(&closure_def_id).copied()
                            {
                                self.net.add_output_arc(closure_start, bb_end, 1);
                                self.net
                                    .add_input_arc(closure_end, self.return_transition, 1);
                            }
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                if let Some((closure_start, closure_end)) =
                                    self.functions_map().get(closure_def_id).copied()
                                {
                                    self.net.add_output_arc(closure_start, bb_end, 1);
                                    self.net
                                        .add_input_arc(closure_end, self.return_transition, 1);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_rayon_join(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
        span: &str,
    ) {
        log::debug!("handle_rayon_join: {:?}", callee_func_name);
        let bb_wait_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "wait_closure");
        let bb_wait_place = Place::new(bb_wait_name, 0, 1, PlaceType::BasicBlock, span.to_string());
        let bb_wait = self.net.add_place(bb_wait_place);

        let bb_join_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "join");
        let bb_join_transition = Transition::new_with_transition_type(
            bb_join_name,
            TransitionType::Join(callee_func_name.to_string()),
        );
        let bb_join = self.net.add_transition(bb_join_transition);

        self.net.add_output_arc(bb_wait, bb_end, 1);
        self.net.add_input_arc(bb_wait, bb_join, 1);

        self.connect_to_target(bb_join, target);

        for arg in args {
            if let Operand::Move(place) | Operand::Copy(place) = &arg.node {
                let place_ty = place.ty(self.body, self.tcx).ty;
                if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                    place_ty.kind()
                {
                    if let Some((closure_start, closure_end)) =
                        self.functions_map().get(&closure_def_id).copied()
                    {
                        self.net.add_output_arc(closure_start, bb_end, 1);
                        self.net.add_input_arc(closure_end, bb_join, 1);
                    }
                }
            }
        }
    }

    fn handle_spawn(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        if let Some(closure_arg) = args.first() {
            match &closure_arg.node {
                Operand::Move(place) | Operand::Copy(place) => {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                        place_ty.kind()
                    {
                        if let Some((closure_start, _)) =
                            self.functions_map().get(&closure_def_id).copied()
                        {
                            self.net.add_output_arc(closure_start, bb_end, 1);
                        }
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            if let Some((closure_start, _)) =
                                self.functions_map().get(&closure_def_id).copied()
                            {
                                self.net.add_output_arc(closure_start, bb_end, 1);
                            }
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                if let Some((closure_start, _)) =
                                    self.functions_map().get(closure_def_id).copied()
                                {
                                    self.net.add_output_arc(closure_start, bb_end, 1);
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(transition) = self.net.get_transition_mut(bb_end) {
            transition.transition_type = TransitionType::Spawn(callee_func_name.to_string());
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) {
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let spawn_def_id = spawn_calls
                .iter()
                .find_map(|(destination, callees)| {
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
                .or_else(|| {
                    log::error!(
                        "No matching spawn call found for join in {:?}",
                        self.instance.def_id()
                    );
                    None
                });

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            self.net.add_input_arc(
                self.functions_map().get(&spawn_def_id.unwrap()).unwrap().1,
                bb_end,
                1,
            );
        }

        self.connect_to_target(bb_end, target);
    }

    fn handle_normal_call(
        &mut self,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: BasicBlock,
        span: &str,
        callee_id: &DefId,
        args: &Box<[Spanned<Operand<'tcx>>]>,
    ) {
        if let Some((callee_start, callee_end)) = self.functions_map().get(callee_id).copied() {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new(bb_wait_name, 0, 1, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_place(bb_wait_place);

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_ret_transition =
                Transition::new_with_transition_type(bb_ret_name, TransitionType::Function);
            let bb_ret = self.net.add_transition(bb_ret_transition);

            self.net.add_output_arc(bb_wait, bb_end, 1);
            self.net.add_input_arc(bb_wait, bb_ret, 1);
            self.net.add_output_arc(callee_start, bb_end, 1);
            match target {
                Some(return_block) => {
                    self.net.add_input_arc(callee_end, bb_ret, 1);
                    self.net
                        .add_output_arc(self.bb_graph.start(*return_block), bb_ret, 1);
                }
                _ => {}
            }
            return;
        } else {
            let name = self.tcx.def_path_str(callee_id);

            for arg in args {
                if let Operand::Copy(place) | Operand::Move(place) = &arg.node {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    match place_ty.kind() {
                        ty::FnDef(closure_def_id, _) | ty::Closure(closure_def_id, _) => {
                            if let Some((callee_start, callee_end)) =
                                self.functions_map().get(&closure_def_id).copied()
                            {
                                let bb_wait_name =
                                    format!("{}_{}_{}", name, bb_idx.index(), "wait");
                                let bb_wait_place = Place::new(
                                    bb_wait_name,
                                    0,
                                    1,
                                    PlaceType::BasicBlock,
                                    span.to_string(),
                                );
                                let bb_wait = self.net.add_place(bb_wait_place);

                                let bb_ret_name =
                                    format!("{}_{}_{}", name, bb_idx.index(), "return");
                                let bb_ret_transition = Transition::new_with_transition_type(
                                    bb_ret_name,
                                    TransitionType::Function,
                                );
                                let bb_ret = self.net.add_transition(bb_ret_transition);

                                self.net.add_output_arc(bb_wait, bb_end, 1);
                                self.net.add_input_arc(bb_wait, bb_ret, 1);
                                self.net.add_output_arc(callee_start, bb_end, 1);
                                match target {
                                    Some(return_block) => {
                                        self.net.add_input_arc(callee_end, bb_ret, 1);
                                        self.net.add_output_arc(
                                            self.bb_graph.start(*return_block),
                                            bb_ret,
                                            1,
                                        );
                                    }
                                    _ => {}
                                }
                                return;
                            }
                        }

                        _ => {}
                    }
                }
            }
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_atomic_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        // FIXME: use regex to match atomic api
        if callee_func_name.contains("::load") {
            if !self.handle_atomic_load(args, bb_end, target, bb_idx, span) {
                log::debug!("no alias found for atomic load in {:?}", span);
                self.connect_to_target(bb_end, target);
            }
            return true;
        } else if callee_func_name.contains("::store") {
            if !self.handle_atomic_store(args, bb_end, target, bb_idx, span) {
                log::debug!("no alias found for atomic store in {:?}", span);
                self.connect_to_target(bb_end, target);
            }
            return true;
        } else if callee_func_name.contains("::compare_exchange") {
            // FIXME: add new petri net model for atomic compare_exchange
            // FIXME: CAS and fetchxx api exist atomic violation???
            false
        } else {
            false
        }
    }

    fn handle_atomic_load(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        let instance_index = self.instance_id.index();
        self.handle_atomic_basic_op(
            "load",
            current_id,
            bb_end,
            target,
            bb_idx,
            span,
            move |alias_id, order, span_str| {
                TransitionType::AtomicLoad(
                    alias_id.clone().into(),
                    order.clone(),
                    span_str,
                    instance_index,
                )
            },
        )
    }

    fn handle_atomic_store(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );
        let instance_index = self.instance_id.index();
        self.handle_atomic_basic_op(
            "store",
            current_id,
            bb_end,
            target,
            bb_idx,
            span,
            move |alias_id, order, span_str| {
                TransitionType::AtomicStore(
                    alias_id.clone().into(),
                    order.clone(),
                    span_str,
                    instance_index,
                )
            },
        )
    }

    fn handle_condvar_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if cfg!(feature = "atomic-violation") {
            return false;
        }

        if has_pn_attribute(self.tcx, callee_def_id, "pn_condvar_notify")
            || self.key_api_regex.condvar_notify.is_match(callee_func_name)
        {
            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            let condvar_alias = condvar_id.get_alias_id();

            for (id, node) in self.resources.condvars().iter() {
                match self.alias.borrow_mut().alias_atomic(condvar_alias, *id) {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        self.net.add_output_arc(*node, bb_end, 1);

                        if let Some(transition) = self.net.get_transition_mut(bb_end) {
                            transition.transition_type = TransitionType::Notify(node.index());
                        }
                        break;
                    }
                    _ => continue,
                }
            }
            self.connect_to_target(bb_end, target);
            true
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_condvar_wait")
            || self.key_api_regex.condvar_wait.is_match(callee_func_name)
        {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new(bb_wait_name, 0, 1, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_place(bb_wait_place);

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "ret");
            let bb_ret_transition =
                Transition::new_with_transition_type(bb_ret_name, TransitionType::Wait);
            let bb_ret = self.net.add_transition(bb_ret_transition);

            self.net.add_output_arc(bb_wait, bb_end, 1);
            self.net.add_input_arc(bb_wait, bb_ret, 1);

            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            let condvar_alias = condvar_id.get_alias_id();

            for (id, node) in self.resources.condvars().iter() {
                match self.alias.borrow_mut().alias_atomic(condvar_alias, *id) {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        self.net.add_input_arc(*node, bb_ret, 1);
                    }
                    _ => continue,
                }
            }

            let guard_id = LockGuardId::new(
                self.instance_id,
                args.get(1).unwrap().node.place().unwrap().local,
            );
            let lock_alias = guard_id.get_alias_id();
            let lock_node = self.resources.locks().get(&lock_alias).unwrap();
            self.net.add_output_arc(*lock_node, bb_end, 1);
            self.net.add_input_arc(*lock_node, bb_ret, 1);

            self.connect_to_target(bb_ret, target);
            true
        } else {
            false
        }
    }

    fn handle_unwind_continue(&mut self, bb_idx: BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "unwind");
        let bb_term_transition = Transition::new_with_transition_type(
            bb_term_name,
            TransitionType::Return(self.instance_id.index()),
        );
        let bb_term_node = self.net.add_transition(bb_term_transition);
        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), bb_term_node, 1);
        self.net.add_output_arc(self.entry_exit.1, bb_term_node, 1);
    }

    fn handle_panic(&mut self, bb_idx: BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "panic");
        let bb_term_transition = Transition::new_with_transition_type(
            bb_term_name,
            TransitionType::Return(self.instance_id.index()),
        );
        let bb_term_node = self.net.add_transition(bb_term_transition);
        self.net
            .add_input_arc(self.bb_graph.last(bb_idx), bb_term_node, 1);
        self.net.add_output_arc(self.entry_exit.1, bb_term_node, 1);
    }

    fn handle_channel_call(
        &mut self,
        callee_def_id: DefId,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
        target: &Option<BasicBlock>,
    ) -> bool {
        if cfg!(feature = "atomic-violation") {
            return false;
        }

        if self.resources.channel_places().is_empty() {
            return false;
        }

        if has_pn_attribute(self.tcx, callee_def_id, "pn_channel_send")
            || self.key_api_regex.channel_send.is_match(callee_func_name)
        {
            let channel_alias = AliasId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.find_channel_place(channel_alias) {
                self.net.add_output_arc(channel_node, bb_end, 1);
                self.connect_to_target(bb_end, target);
                return true;
            }
        } else if has_pn_attribute(self.tcx, callee_def_id, "pn_channel_recv")
            || self.key_api_regex.channel_recv.is_match(callee_func_name)
        {
            let channel_alias = AliasId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.find_channel_place(channel_alias) {
                self.net.add_input_arc(channel_node, bb_end, 1);
                self.connect_to_target(bb_end, target);
                return true;
            }
        }

        false
    }

    fn handle_call(
        &mut self,
        bb_idx: BasicBlock,
        func: &Operand<'tcx>,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        name: &str,
        span: &str,
        unwind: &UnwindAction,
    ) {
        match (target, unwind) {
            (None, UnwindAction::Continue) => {
                self.handle_unwind_continue(bb_idx, name);
                return;
            }
            (Some(t), _) => {
                if self.body.basic_blocks[*t].is_cleanup {
                    self.handle_panic(bb_idx, name);
                    return;
                }
            }
            _ => {}
        }

        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "call");
        let bb_end = self.create_call_transition(bb_idx, &bb_term_name);
        let callee_ty = func.ty(self.body, self.tcx);
        let callee_def_id = match callee_ty.kind() {
            rustc_middle::ty::TyKind::FnPtr(..) => {
                log::debug!("call fnptr: {:?}", callee_ty);
                self.connect_to_target(bb_end, target);
                return;
            }
            rustc_middle::ty::TyKind::FnDef(id, _) | rustc_middle::ty::TyKind::Closure(id, _) => {
                *id
            }
            _ => {
                panic!("TyKind::FnDef, a function definition, but got: {callee_ty:?}");
            }
        };

        let callee_func_name = format_name(callee_def_id);

        if let Some(_) = self.handle_lock_call(destination, target, bb_end) {
            log::debug!("callee_func_name with lock: {:?}", callee_func_name);
            return;
        }

        if self.handle_condvar_call(
            callee_def_id,
            &callee_func_name,
            args,
            bb_end,
            target,
            name,
            &bb_idx,
            span,
        ) {
            log::debug!("callee_func_name with condvar: {:?}", callee_func_name);
            return;
        }

        if callee_func_name.contains("::drop") && !cfg!(feature = "atomic-violation") {
            log::debug!("callee_func_name with drop: {:?}", callee_func_name);
            let lockguard_id = LockGuardId::new(
                self.instance_id,
                args.get(0).unwrap().node.place().unwrap().local,
            );
            if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                let lock_alias = lockguard_id.get_alias_id();
                let lock_node = self.resources.locks().get(&lock_alias).unwrap();
                match &self.lockguards[&lockguard_id].lockguard_ty {
                    LockGuardTy::StdMutex(_)
                    | LockGuardTy::ParkingLotMutex(_)
                    | LockGuardTy::SpinMutex(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);

                        match self.net.get_transition_mut(bb_end) {
                            Some(transition) => {
                                transition.transition_type =
                                    TransitionType::Unlock(lock_node.index());
                            }
                            _ => {}
                        }
                    }

                    LockGuardTy::StdRwLockRead(_)
                    | LockGuardTy::ParkingLotRead(_)
                    | LockGuardTy::SpinRead(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);

                        match self.net.get_transition_mut(bb_end) {
                            Some(transition) => {
                                transition.transition_type =
                                    TransitionType::Unlock(lock_node.index());
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        self.net.add_output_arc(*lock_node, bb_end, 10);
                        match self.net.get_transition_mut(bb_end) {
                            Some(transition) => {
                                transition.transition_type =
                                    TransitionType::Unlock(lock_node.index());
                            }
                            _ => {}
                        }
                    }
                }
            }
            self.connect_to_target(bb_end, target);
            return;
        }

        if self.handle_channel_call(callee_def_id, &callee_func_name, args, bb_end, target) {
            log::debug!("callee_func_name with channel: {:?}", callee_func_name);
            return;
        }

        if self.handle_thread_call(
            callee_def_id,
            &callee_func_name,
            args,
            target,
            bb_end,
            &bb_idx,
            span,
        ) {
            log::debug!("callee_func_name with thread: {:?}", callee_func_name);
            return;
        }

        if self.handle_atomic_call(&callee_func_name, args, bb_end, target, &bb_idx, span) {
            log::debug!("callee_func_name with atomic: {:?}", callee_func_name);
            return;
        }

        log::debug!("callee_func_name with normal: {:?}", callee_func_name);
        if callee_func_name.contains("core::panic") {
            self.net.add_output_arc(self.entry_exit.1, bb_end, 1);
            return;
        }
        self.handle_normal_call(bb_end, target, name, bb_idx, span, &callee_def_id, args);
    }

    fn handle_drop(
        &mut self,
        bb_idx: &BasicBlock,
        place: &rustc_middle::mir::Place<'tcx>,
        target: &BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "drop");
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Drop);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net
            .add_input_arc(self.bb_graph.last(*bb_idx), bb_end, 1);

        if cfg!(feature = "atomic-violation") {
            self.net
                .add_output_arc(self.bb_graph.start(*target), bb_end, 1);
            return;
        }

        if !bb.is_cleanup {
            let lockguard_id = LockGuardId::new(self.instance_id, place.local);

            if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                let lock_alias = lockguard_id.get_alias_id();
                let lock_node = self.resources.locks().get(&lock_alias).unwrap();
                match &self.lockguards[&lockguard_id].lockguard_ty {
                    LockGuardTy::StdMutex(_)
                    | LockGuardTy::ParkingLotMutex(_)
                    | LockGuardTy::SpinMutex(_)
                    | LockGuardTy::StdRwLockRead(_)
                    | LockGuardTy::ParkingLotRead(_)
                    | LockGuardTy::SpinRead(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);
                    }
                    _ => {
                        self.net.add_output_arc(*lock_node, bb_end, 10);
                    }
                }

                match self.net.get_transition_mut(bb_end) {
                    Some(transition) => {
                        transition.transition_type = TransitionType::Unlock(lock_node.index());
                    }
                    _ => {}
                }
            }
        }

        self.net
            .add_output_arc(self.bb_graph.start(*target), bb_end, 1);
    }

    fn has_unsafe_alias(&self, place_id: AliasId) -> (bool, PlaceId, Option<AliasId>) {
        for (unsafe_place, node_index) in self.resources.unsafe_places().iter() {
            match self
                .alias
                .borrow_mut()
                .alias_atomic(place_id, *unsafe_place)
            {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    return (true, *node_index, Some(*unsafe_place));
                }
                _ => return (false, PlaceId::new(0), None),
            }
        }
        (false, PlaceId::new(0), None)
    }

    fn visit_statement_body(&mut self, statement: &Statement<'tcx>, bb_idx: BasicBlock) {
        let span_str = format!("{:?}", statement.source_info.span);
        if let StatementKind::Assign(box (dest, rvalue)) = &statement.kind {
            let fn_name = self.tcx.def_path_str(self.instance.def_id());

            self.process_rvalue_reads(rvalue, &fn_name, bb_idx, &span_str);

            self.process_place_writes(dest, &fn_name, bb_idx, &span_str);
        }
    }

    fn process_rvalue_reads(
        &mut self,
        rvalue: &Rvalue<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let places = match rvalue {
            Rvalue::Use(operand) => match operand {
                Operand::Move(place) | Operand::Copy(place) => vec![place],
                Operand::Constant(_) => vec![],
            },
            Rvalue::BinaryOp(_, box (op1, op2)) => {
                let mut places = Vec::new();
                if let Operand::Move(place) | Operand::Copy(place) = op1 {
                    places.push(place);
                }
                if let Operand::Move(place) | Operand::Copy(place) = op2 {
                    places.push(place);
                }
                places
            }
            Rvalue::Ref(_, _, place) => {
                vec![place]
            }
            Rvalue::Discriminant(place) => {
                vec![place]
            }
            Rvalue::Aggregate(_, operands) => operands
                .iter()
                .filter_map(|op| match op {
                    Operand::Move(place) | Operand::Copy(place) => Some(place),
                    _ => None,
                })
                .collect(),

            _ => vec![],
        };

        for place in places {
            let place_id = AliasId::new(self.instance_id, place.local);
            let place_ty = format!("{:?}", place.ty(self.body, self.tcx));

            let alias_result = self.has_unsafe_alias(place_id);
            if alias_result.0 {
                let transition_name =
                    format!("{}_read_{:?}_in:{}", fn_name, place_id.local, span_str);
                let read_t = Transition::new_with_transition_type(
                    transition_name.clone(),
                    TransitionType::UnsafeRead(
                        alias_result.1.index(),
                        span_str.to_string(),
                        bb_idx.index(),
                        place_ty,
                    ),
                );
                let unsafe_read_t = self.net.add_transition(read_t);

                let last_node = self.bb_graph.last(bb_idx);
                self.net.add_input_arc(last_node, unsafe_read_t, 1);

                let unsafe_place = alias_result.1;
                self.net.add_output_arc(unsafe_place, unsafe_read_t, 1);
                self.net.add_input_arc(unsafe_place, unsafe_read_t, 1);

                let place_name = format!("{}_rready", &transition_name.as_str());
                let temp_place = Place::new(
                    place_name,
                    0,
                    1,
                    PlaceType::BasicBlock,
                    span_str.to_string(),
                );
                let temp_place_node = self.net.add_place(temp_place);
                self.net.add_output_arc(temp_place_node, unsafe_read_t, 1);

                self.bb_graph.push(bb_idx, temp_place_node);
            }
        }
    }

    fn process_place_writes(
        &mut self,
        place: &rustc_middle::mir::Place<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let place_id = AliasId::new(self.instance_id, place.local);
        let place_ty = format!("{:?}", place.ty(self.body, self.tcx));

        let alias_result = self.has_unsafe_alias(place_id);
        if alias_result.0 {
            let transition_name = format!("{}_write_{:?}_in:{}", fn_name, place_id.local, span_str);
            let write_t = Transition::new_with_transition_type(
                transition_name.clone(),
                TransitionType::UnsafeWrite(
                    alias_result.1.index(),
                    span_str.to_string(),
                    bb_idx.index(),
                    place_ty,
                ),
            );
            let unsafe_write_t = self.net.add_transition(write_t);

            let last_node = self.bb_graph.last(bb_idx);
            self.net.add_input_arc(last_node, unsafe_write_t, 1);

            let unsafe_place = alias_result.1;
            self.net.add_output_arc(unsafe_place, unsafe_write_t, 1);
            self.net.add_input_arc(unsafe_place, unsafe_write_t, 1);

            let place_name = format!("{}_wready", &transition_name.as_str());
            let temp_place = Place::new(
                place_name,
                0,
                1,
                PlaceType::BasicBlock,
                span_str.to_string(),
            );
            let temp_place_node = self.net.add_place(temp_place);
            self.net.add_output_arc(temp_place_node, unsafe_write_t, 1);

            self.bb_graph.push(bb_idx, temp_place_node);
        }
    }
}

impl<'translate, 'analysis, 'tcx> Visitor<'tcx> for BodyToPetriNet<'translate, 'analysis, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let def_id = self.instance.def_id();

        let fn_name = self.tcx.def_path_str(def_id);

        if fn_name.contains("::deserialize")
            || fn_name.contains("::serialize")
            || fn_name.contains("::visit_seq")
            || fn_name.contains("::visit_map")
        {
            log::warn!("Skipping serialization function: {}", fn_name);
            return;
        }

        self.init_basic_block(body, &fn_name);

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                continue;
            }

            for stmt in bb.statements.iter() {
                if let Some(ref term) = bb.terminator {
                    if let TerminatorKind::Assert { .. } = &term.kind {
                        break;
                    }
                }
                self.visit_statement_body(stmt, bb_idx);
            }

            if bb_idx.index() == 0 {
                self.handle_start_block(&fn_name, bb_idx, def_id);
            }

            if let Some(term) = &bb.terminator {
                self.handle_terminator(term, bb_idx, &fn_name, bb);
            }
        }
    }
}
