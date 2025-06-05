use super::{
    callgraph::{CallGraph, InstanceId},
    net_structure::{
        CallType, ControlType, DropType, KeyApiRegex, NetConfig, PetriNetEdge, PetriNetNode, Place,
        PlaceType,
    },
};
use crate::{
    concurrency::{
        atomic::AtomicOrdering,
        blocking::{CondVarId, LockGuardId, LockGuardMap, LockGuardTy},
        channel::ChannelId,
    },
    graph::net_structure::Transition,
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    util::format_name,
};
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{
        visit::Visitor, BasicBlock, BasicBlockData, Const, Operand, Rvalue, Statement,
        StatementKind, SwitchTargets, TerminatorKind, UnwindAction,
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

pub struct BodyToPetriNet<'translate, 'analysis, 'tcx> {
    instance_id: InstanceId,
    instance: &'translate Instance<'tcx>,
    body: &'translate Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    callgraph: &'translate CallGraph<'tcx>,
    pub net: &'translate mut Graph<PetriNetNode, PetriNetEdge>,
    alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
    pub lockguards: LockGuardMap<'tcx>,
    function_counter: &'translate HashMap<DefId, (NodeIndex, NodeIndex)>,
    locks_counter: &'translate HashMap<LockGuardId, NodeIndex>,
    bb_node_start_end: HashMap<BasicBlock, NodeIndex>,
    bb_node_vec: HashMap<BasicBlock, Vec<NodeIndex>>,
    condvar_id: &'translate HashMap<CondVarId, NodeIndex>,
    atomic_places: &'translate HashMap<AliasId, NodeIndex>,
    atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
    pub exclude_bb: HashSet<usize>,
    return_transition: NodeIndex,
    entry_exit: (NodeIndex, NodeIndex),
    unsafe_places: &'translate HashMap<AliasId, NodeIndex>,
    key_api_regex: &'translate KeyApiRegex,
    net_config: &'translate NetConfig,
    channel_places: &'translate HashMap<ChannelId, NodeIndex>,
}

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        callgraph: &'translate CallGraph<'tcx>,
        net: &'translate mut Graph<PetriNetNode, PetriNetEdge>,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'translate HashMap<DefId, (NodeIndex, NodeIndex)>,
        locks_counter: &'translate HashMap<LockGuardId, NodeIndex>,
        condvar_id: &'translate HashMap<CondVarId, NodeIndex>,
        atomic_places: &'translate HashMap<AliasId, NodeIndex>,
        atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
        entry_exit: (NodeIndex, NodeIndex),
        unsafe_places: &'translate HashMap<AliasId, NodeIndex>,
        key_api_regex: &'translate KeyApiRegex,
        net_config: &'translate NetConfig,
        channel_places: &'translate HashMap<ChannelId, NodeIndex>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            callgraph,
            net,
            alias,
            lockguards,
            function_counter,
            locks_counter,
            bb_node_start_end: HashMap::default(),
            bb_node_vec: HashMap::new(),
            condvar_id,
            atomic_places,
            atomic_order_maps,
            exclude_bb: HashSet::new(),
            return_transition: NodeIndex::new(0),
            entry_exit,
            unsafe_places,
            key_api_regex,
            net_config,
            channel_places,
        }
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
            let bb_start_place = Place::new_with_span(bb_name, 0u8, PlaceType::BasicBlock, bb_span);
            let bb_start = self.net.add_node(PetriNetNode::P(bb_start_place));
            self.bb_node_start_end
                .insert(bb_idx.clone(), bb_start.clone());
            self.bb_node_vec.insert(bb_idx.clone(), vec![bb_start]);
        }
    }

    fn handle_start_block(&mut self, name: &str, bb_idx: BasicBlock, def_id: DefId) {
        let bb_start_name = format!("{}_{}_start", name, bb_idx.index());
        let bb_start_transition =
            Transition::new(bb_start_name, ControlType::Start(self.instance_id));
        let bb_start = self.net.add_node(PetriNetNode::T(bb_start_transition));

        self.net.add_edge(
            self.function_counter.get(&def_id).unwrap().0,
            bb_start,
            PetriNetEdge { label: 1 },
        );
        self.net.add_edge(
            bb_start,
            *self.bb_node_start_end.get(&bb_idx).unwrap(),
            PetriNetEdge { label: 1 },
        );
    }

    fn handle_assert(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "assert");
        let bb_term_transition = Transition::new(bb_term_name, ControlType::Assert);
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            PetriNetEdge { label: 1 },
        );
        self.net.add_edge(
            bb_end,
            *self.bb_node_start_end.get(target).unwrap(),
            PetriNetEdge { label: 1 },
        );
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
            TerminatorKind::Unreachable => {
                todo!("unreachable")
            }
            _ => {}
        }
    }

    fn handle_goto(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        if self.body.basic_blocks[*target].is_cleanup {
            self.handle_panic(bb_idx, name);
            return;
        }

        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "goto");
        let bb_term_transition = Transition::new(bb_term_name, ControlType::Goto);
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            PetriNetEdge { label: 1u8 },
        );

        let target_bb_start = self.bb_node_start_end.get(&target).unwrap();
        self.net
            .add_edge(bb_end, *target_bb_start, PetriNetEdge { label: 1u8 });
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
            let bb_term_transition = Transition::new(bb_term_name, ControlType::Switch);
            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

            self.net.add_edge(
                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                bb_end,
                PetriNetEdge { label: 1u8 },
            );
            let target_bb_start = self.bb_node_start_end.get(t).unwrap();
            self.net
                .add_edge(bb_end, *target_bb_start, PetriNetEdge { label: 1u8 });
        }
    }

    fn handle_return(&mut self, bb_idx: BasicBlock, name: &str) {
        let return_node = self
            .function_counter
            .get(&self.instance.def_id())
            .unwrap()
            .1;

        if self.return_transition.index() == 0 {
            let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_term_transition =
                Transition::new(bb_term_name, ControlType::Return(self.instance_id));
            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

            self.return_transition = bb_end;
        }

        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            self.return_transition,
            PetriNetEdge { label: 1u8 },
        );
        self.net.add_edge(
            self.return_transition,
            return_node,
            PetriNetEdge { label: 1u8 },
        );
    }

    fn create_call_transition(&mut self, bb_idx: BasicBlock, bb_term_name: &str) -> NodeIndex {
        let bb_term_transition = Transition::new(
            bb_term_name.to_string(),
            ControlType::Call(CallType::Function),
        );
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            PetriNetEdge { label: 1u8 },
        );
        bb_end
    }

    fn handle_lock_call(
        &mut self,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) -> Option<CallType> {
        let lockguard_id = LockGuardId::new(self.instance_id, destination.local);
        if let Some(guard) = self.lockguards.get_mut(&lockguard_id) {
            let lock_node = self.locks_counter.get(&lockguard_id).unwrap();

            let call_type = match &guard.lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => CallType::Lock(lock_node.clone()),
                LockGuardTy::StdRwLockRead(_)
                | LockGuardTy::ParkingLotRead(_)
                | LockGuardTy::SpinRead(_) => CallType::RwLockRead(lock_node.clone()),
                _ => CallType::RwLockWrite(lock_node.clone()),
            };

            self.update_lock_transition(bb_end, lock_node, &call_type);
            self.connect_to_target(bb_end, target);
            Some(call_type)
        } else {
            None
        }
    }

    fn update_lock_transition(
        &mut self,
        bb_end: NodeIndex,
        lock_node: &NodeIndex,
        call_type: &CallType,
    ) {
        if let Some(PetriNetNode::T(transition)) = self.net.node_weight_mut(bb_end) {
            transition.transition_type = ControlType::Call(call_type.clone());
        }

        match call_type {
            CallType::Lock(_) | CallType::RwLockRead(_) => {
                self.net
                    .add_edge(*lock_node, bb_end, PetriNetEdge { label: 1u8 });
            }
            CallType::RwLockWrite(_) => {
                self.net
                    .add_edge(*lock_node, bb_end, PetriNetEdge { label: 10u8 });
            }
            _ => {}
        }
    }

    fn connect_to_target(&mut self, bb_end: NodeIndex, target: &Option<BasicBlock>) {
        if let Some(target_bb) = target {
            self.net.add_edge(
                bb_end,
                *self.bb_node_vec.get(target_bb).unwrap().first().unwrap(),
                PetriNetEdge { label: 1u8 },
            );
        }
    }

    fn handle_thread_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if self.key_api_regex.thread_spawn.is_match(callee_func_name) {
            self.handle_spawn(callee_func_name, args, target, bb_end);
            true
        } else if self.key_api_regex.scope_join.is_match(callee_func_name) {
            self.handle_scope_join(callee_func_name, bb_idx, args, target, bb_end, span);
            true
        } else if self.key_api_regex.thread_join.is_match(callee_func_name) {
            self.handle_join(callee_func_name, args, target, bb_end);
            true
        } else if callee_func_name.contains("rayon_core::join") {
            self.handle_rayon_join(callee_func_name, bb_idx, args, target, bb_end, span);
            true
        } else if self.key_api_regex.scope_spwan.is_match(callee_func_name) {
            self.handle_scope_spawn(callee_func_name, bb_idx, args, target, bb_end, span);
            true
        } else {
            false
        }
    }

    fn handle_scope_join(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
        span: &str,
    ) {
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let spawn_def_id = spawn_calls
                .iter()
                .find_map(|(def_id, local)| {
                    let spawn_local_id = AliasId::new(self.instance_id, *local);
                    match self
                        .alias
                        .borrow_mut()
                        .alias(join_id.into(), spawn_local_id.into())
                    {
                        ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                            Some(*def_id)
                        }
                        _ => None,
                    }
                })
                .or_else(|| {
                    log::error!(
                        "No matching spawn call found for join in {:?}",
                        self.instance.def_id()
                    );
                    None
                });

            if let Some(PetriNetNode::T(transition)) = self.net.node_weight_mut(bb_end) {
                transition.transition_type =
                    ControlType::Call(CallType::Join(callee_func_name.to_string()));
            }

            self.net.add_edge(
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
                bb_end,
                PetriNetEdge { label: 1 },
            );
            self.net.add_edge(
                bb_end,
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
                PetriNetEdge { label: 1 },
            );
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_scope_spawn(
        &mut self,
        callee_func_name: &str,
        bb_idx: &BasicBlock,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
        span: &str,
    ) {
        if self.return_transition.index() == 0 {
            let bb_term_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "return");
            let bb_term_transition =
                Transition::new(bb_term_name, ControlType::Call(CallType::Function));
            self.return_transition = self.net.add_node(PetriNetNode::T(bb_term_transition));
        }

        if let Some(closure_arg) = args.get(1) {
            match &closure_arg.node {
                Operand::Move(place) | Operand::Copy(place) => {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                        place_ty.kind()
                    {
                        self.net.add_edge(
                            bb_end,
                            self.function_counter.get(&closure_def_id).unwrap().0,
                            PetriNetEdge { label: 1u8 },
                        );
                        self.net.add_edge(
                            self.function_counter.get(&closure_def_id).unwrap().1,
                            self.return_transition,
                            PetriNetEdge { label: 1u8 },
                        );
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            self.net.add_edge(
                                bb_end,
                                self.function_counter.get(&closure_def_id).unwrap().0,
                                PetriNetEdge { label: 1u8 },
                            );
                            self.net.add_edge(
                                self.function_counter.get(&closure_def_id).unwrap().1,
                                self.return_transition,
                                PetriNetEdge { label: 1u8 },
                            );
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                self.net.add_edge(
                                    bb_end,
                                    self.function_counter.get(closure_def_id).unwrap().0,
                                    PetriNetEdge { label: 1u8 },
                                );
                                self.net.add_edge(
                                    self.function_counter.get(&closure_def_id).unwrap().1,
                                    self.return_transition,
                                    PetriNetEdge { label: 1u8 },
                                );
                            }
                        }
                    }
                }
                _ => {}
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
        bb_end: NodeIndex,
        span: &str,
    ) {
        log::debug!("handle_rayon_join: {:?}", callee_func_name);
        let bb_wait_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "wait_closure");
        let bb_wait_place =
            Place::new_with_span(bb_wait_name, 0, PlaceType::BasicBlock, span.to_string());
        let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

        let bb_join_name = format!("{}_{}_{}", callee_func_name, bb_idx.index(), "join");
        let bb_join_transition = Transition::new(
            bb_join_name,
            ControlType::Call(CallType::Join(callee_func_name.to_string())),
        );
        let bb_join = self.net.add_node(PetriNetNode::T(bb_join_transition));

        self.net
            .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1u8 });
        self.net
            .add_edge(bb_wait, bb_join, PetriNetEdge { label: 1u8 });

        self.connect_to_target(bb_join, target);

        for arg in args {
            if let Operand::Move(place) | Operand::Copy(place) = &arg.node {
                let place_ty = place.ty(self.body, self.tcx).ty;
                if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                    place_ty.kind()
                {
                    self.net.add_edge(
                        bb_end,
                        self.function_counter.get(&closure_def_id).unwrap().0,
                        PetriNetEdge { label: 1u8 },
                    );

                    self.net.add_edge(
                        self.function_counter.get(&closure_def_id).unwrap().1,
                        bb_join,
                        PetriNetEdge { label: 1u8 },
                    );
                }
            }
        }
    }

    fn handle_spawn(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) {
        if let Some(closure_arg) = args.first() {
            match &closure_arg.node {
                Operand::Move(place) | Operand::Copy(place) => {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                        place_ty.kind()
                    {
                        self.net.add_edge(
                            bb_end,
                            self.function_counter.get(&closure_def_id).unwrap().0,
                            PetriNetEdge { label: 1u8 },
                        );
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            self.net.add_edge(
                                bb_end,
                                self.function_counter.get(&closure_def_id).unwrap().0,
                                PetriNetEdge { label: 1u8 },
                            );
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                self.net.add_edge(
                                    bb_end,
                                    self.function_counter.get(closure_def_id).unwrap().0,
                                    PetriNetEdge { label: 1u8 },
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        match self.net.node_weight_mut(bb_end) {
            Some(PetriNetNode::T(t)) => {
                t.transition_type =
                    ControlType::Call(CallType::Spawn(callee_func_name.to_string()));
            }
            _ => {}
        }
        self.connect_to_target(bb_end, target);
    }

    fn handle_join(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) {
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        if let Some(spawn_calls) = self.callgraph.get_spawn_calls(self.instance.def_id()) {
            let spawn_def_id = spawn_calls
                .iter()
                .find_map(|(def_id, local)| {
                    let spawn_local_id = AliasId::new(self.instance_id, *local);
                    match self
                        .alias
                        .borrow_mut()
                        .alias(join_id.into(), spawn_local_id.into())
                    {
                        ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                            Some(*def_id)
                        }
                        _ => None,
                    }
                })
                .or_else(|| {
                    log::error!(
                        "No matching spawn call found for join in {:?}",
                        self.instance.def_id()
                    );
                    None
                });

            if let Some(PetriNetNode::T(transition)) = self.net.node_weight_mut(bb_end) {
                transition.transition_type =
                    ControlType::Call(CallType::Join(callee_func_name.to_string()));
            }

            self.net.add_edge(
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
                bb_end,
                PetriNetEdge { label: 1 },
            );
        }

        self.connect_to_target(bb_end, target);
    }

    fn handle_normal_call(
        &mut self,
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: BasicBlock,
        span: &str,
        callee_id: &DefId,
        args: &Box<[Spanned<Operand<'tcx>>]>,
    ) {
        if let Some((callee_start, callee_end)) = self.function_counter.get(callee_id) {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new_with_span(bb_wait_name, 0, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_ret_transition =
                Transition::new(bb_ret_name, ControlType::Call(CallType::Function));
            let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

            self.net
                .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1u8 });
            self.net
                .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1u8 });
            self.net
                .add_edge(bb_end, *callee_start, PetriNetEdge { label: 1u8 });
            match target {
                Some(return_block) => {
                    self.net
                        .add_edge(*callee_end, bb_ret, PetriNetEdge { label: 1u8 });
                    self.net.add_edge(
                        bb_ret,
                        *self.bb_node_start_end.get(return_block).unwrap(),
                        PetriNetEdge { label: 1u8 },
                    );
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
                                self.function_counter.get(&closure_def_id)
                            {
                                let bb_wait_name =
                                    format!("{}_{}_{}", name, bb_idx.index(), "wait");
                                let bb_wait_place = Place::new_with_span(
                                    bb_wait_name,
                                    0,
                                    PlaceType::BasicBlock,
                                    span.to_string(),
                                );
                                let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                                let bb_ret_name =
                                    format!("{}_{}_{}", name, bb_idx.index(), "return");
                                let bb_ret_transition = Transition::new(
                                    bb_ret_name,
                                    ControlType::Call(CallType::Function),
                                );
                                let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

                                self.net
                                    .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1u8 });
                                self.net
                                    .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1u8 });
                                self.net.add_edge(
                                    bb_end,
                                    *callee_start,
                                    PetriNetEdge { label: 1u8 },
                                );
                                match target {
                                    Some(return_block) => {
                                        self.net.add_edge(
                                            *callee_end,
                                            bb_ret,
                                            PetriNetEdge { label: 1u8 },
                                        );
                                        self.net.add_edge(
                                            bb_ret,
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1u8 },
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
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
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
            self.handle_atomic_compare_exchange(args, bb_end, target, bb_idx, span)
        } else {
            false
        }
    }

    fn handle_atomic_load(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        for atomic_e in self.atomic_places.iter() {
            if !matches!(
                self.alias
                    .borrow_mut()
                    .alias_atomic(current_id.into(), atomic_e.0.clone().into()),
                ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably
            ) {
                continue;
            }

            let atomic_load_place = Place::new_with_span(
                format!(
                    "atomic_load_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_load_place_node = self.net.add_node(PetriNetNode::P(atomic_load_place));
            self.net
                .add_edge(bb_end, atomic_load_place_node, PetriNetEdge { label: 1 });

            if let Some(order) = self.atomic_order_maps.get(&current_id) {
                let atomic_load_transition = Transition::new(
                    format!(
                        "atomic_{:?}_load_{:?}_{:?}",
                        self.instance_id.index(),
                        order,
                        bb_idx.index()
                    ),
                    ControlType::Call(CallType::AtomicLoad(
                        atomic_e.0.clone().into(),
                        order.clone(),
                        span.to_string(),
                        self.instance_id,
                    )),
                );
                let atomic_load_transition_node =
                    self.net.add_node(PetriNetNode::T(atomic_load_transition));

                self.net.add_edge(
                    atomic_load_place_node,
                    atomic_load_transition_node,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    atomic_load_transition_node,
                    *atomic_e.1,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    *atomic_e.1,
                    atomic_load_transition_node,
                    PetriNetEdge { label: 1 },
                );

                if let Some(t) = target {
                    self.net.add_edge(
                        atomic_load_transition_node,
                        *self.bb_node_start_end.get(t).unwrap(),
                        PetriNetEdge { label: 1 },
                    );
                }
            }
            return true;
        }
        false
    }

    fn handle_atomic_store(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );
        for atomic_e in self.atomic_places.iter() {
            if !matches!(
                self.alias
                    .borrow_mut()
                    .alias_atomic(current_id.into(), atomic_e.0.clone().into()),
                ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably
            ) {
                continue;
            }

            let atomic_store_place = Place::new_with_span(
                format!(
                    "atomic_store_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_store_place_node = self.net.add_node(PetriNetNode::P(atomic_store_place));
            self.net
                .add_edge(bb_end, atomic_store_place_node, PetriNetEdge { label: 1 });

            if let Some(order) = self.atomic_order_maps.get(&current_id) {
                let atomic_store_transition = Transition::new(
                    format!(
                        "atomic_{:?}_store_{:?}_{:?}",
                        self.instance_id.index(),
                        order,
                        bb_idx.index()
                    ),
                    ControlType::Call(CallType::AtomicStore(
                        atomic_e.0.clone().into(),
                        order.clone(),
                        span.to_string(),
                        self.instance_id,
                    )),
                );
                let atomic_store_transition_node =
                    self.net.add_node(PetriNetNode::T(atomic_store_transition));

                self.net.add_edge(
                    atomic_store_place_node,
                    atomic_store_transition_node,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    atomic_store_transition_node,
                    *atomic_e.1,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    *atomic_e.1,
                    atomic_store_transition_node,
                    PetriNetEdge { label: 1 },
                );

                if let Some(t) = target {
                    self.net.add_edge(
                        atomic_store_transition_node,
                        *self.bb_node_start_end.get(t).unwrap(),
                        PetriNetEdge { label: 1 },
                    );
                }
            }
            return true;
        }
        false
    }

    fn handle_atomic_compare_exchange(
        &mut self,
        args: &[Spanned<Operand<'tcx>>],
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        let current_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        for atomic_e in self.atomic_places.iter() {
            if !matches!(
                self.alias
                    .borrow_mut()
                    .alias(current_id.into(), atomic_e.0.clone().into()),
                ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably
            ) {
                continue;
            }

            log::info!("atomic compare_exchange: {:?}", atomic_e.0);

            let atomic_cmpxchg_place = Place::new_with_span(
                format!(
                    "atomic_cmpxchg_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_cmpxchg_place_node =
                self.net.add_node(PetriNetNode::P(atomic_cmpxchg_place));
            self.net
                .add_edge(bb_end, atomic_cmpxchg_place_node, PetriNetEdge { label: 1 });

            if let (Some(success_order), Some(failure_order)) = (
                self.atomic_order_maps.get(&current_id),
                self.atomic_order_maps.get(&AliasId::new(
                    self.instance_id,
                    args.get(1).unwrap().node.place().unwrap().local,
                )),
            ) {
                let atomic_cmpxchg_transition = Transition::new(
                    format!(
                        "atomic_{:?}_cmpxchg_{:?}_{:?}",
                        self.instance_id.index(),
                        success_order,
                        bb_idx.index()
                    ),
                    ControlType::Call(CallType::AtomicCmpXchg(
                        atomic_e.0.clone().into(),
                        success_order.clone(),
                        failure_order.clone(),
                        span.to_string(),
                        self.instance_id,
                    )),
                );
                let atomic_cmpxchg_transition_node = self
                    .net
                    .add_node(PetriNetNode::T(atomic_cmpxchg_transition));

                self.net.add_edge(
                    atomic_cmpxchg_place_node,
                    atomic_cmpxchg_transition_node,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    atomic_cmpxchg_transition_node,
                    *atomic_e.1,
                    PetriNetEdge { label: 1 },
                );
                self.net.add_edge(
                    *atomic_e.1,
                    atomic_cmpxchg_transition_node,
                    PetriNetEdge { label: 1 },
                );

                if let Some(t) = target {
                    self.net.add_edge(
                        atomic_cmpxchg_transition_node,
                        *self.bb_node_start_end.get(t).unwrap(),
                        PetriNetEdge { label: 1 },
                    );
                }
            }
            return true;
        }
        true
    }

    fn handle_condvar_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if self.key_api_regex.condvar_notify.is_match(callee_func_name) {
            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            for (id, node) in self.condvar_id.iter() {
                match self
                    .alias
                    .borrow_mut()
                    .alias_atomic(condvar_id.into(), (*id).into())
                {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        self.net.add_edge(bb_end, *node, PetriNetEdge { label: 1 });

                        if let Some(PetriNetNode::T(t)) = self.net.node_weight_mut(bb_end) {
                            t.transition_type = ControlType::Call(CallType::Notify(*node));
                        }
                        break;
                    }
                    _ => continue,
                }
            }
            self.connect_to_target(bb_end, target);
            true
        } else if self.key_api_regex.condvar_wait.is_match(callee_func_name) {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new_with_span(bb_wait_name, 0, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "ret");
            let bb_ret_transition = Transition::new(bb_ret_name, ControlType::Call(CallType::Wait));
            let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

            self.net
                .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1 });
            self.net
                .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1 });

            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            for (id, node) in self.condvar_id.iter() {
                match self
                    .alias
                    .borrow_mut()
                    .alias_atomic(condvar_id.into(), (*id).into())
                {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        self.net.add_edge(*node, bb_ret, PetriNetEdge { label: 1 });
                    }
                    _ => continue,
                }
            }

            let guard_id = LockGuardId::new(
                self.instance_id,
                args.get(1).unwrap().node.place().unwrap().local,
            );
            let lock_node = self.locks_counter.get(&guard_id).unwrap();
            self.net
                .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1 });
            self.net
                .add_edge(*lock_node, bb_ret, PetriNetEdge { label: 1 });

            self.connect_to_target(bb_ret, target);
            true
        } else {
            false
        }
    }

    fn handle_unwind_continue(&mut self, bb_idx: BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "unwind");
        let bb_term_transition =
            Transition::new(bb_term_name, ControlType::Return(self.instance_id));
        let bb_term_node = self.net.add_node(PetriNetNode::T(bb_term_transition));
        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_term_node,
            PetriNetEdge { label: 1 },
        );
        self.net
            .add_edge(bb_term_node, self.entry_exit.1, PetriNetEdge { label: 1 });
    }

    fn handle_panic(&mut self, bb_idx: BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "panic");
        let bb_term_transition =
            Transition::new(bb_term_name, ControlType::Return(self.instance_id));
        let bb_term_node = self.net.add_node(PetriNetNode::T(bb_term_transition));
        self.net.add_edge(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_term_node,
            PetriNetEdge { label: 1 },
        );
        self.net
            .add_edge(bb_term_node, self.entry_exit.1, PetriNetEdge { label: 1 });
    }

    fn handle_channel_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
    ) -> bool {
        if self.channel_places.is_empty() {
            return false;
        }

        if self.key_api_regex.channel_send.is_match(callee_func_name) {
            let channel_id = ChannelId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.channel_places.iter().find(|(id, _)| {
                match self
                    .alias
                    .borrow_mut()
                    .alias_atomic(channel_id.clone().into(), (**id).clone().into())
                {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => true,
                    _ => false,
                }
            }) {
                self.net
                    .add_edge(bb_end, *channel_node.1, PetriNetEdge { label: 1 });
                self.connect_to_target(bb_end, target);
                return true;
            }
        } else if self.key_api_regex.channel_recv.is_match(callee_func_name) {
            let channel_id = ChannelId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );

            if let Some(channel_node) = self.channel_places.iter().find(|(id, _)| {
                match self
                    .alias
                    .borrow_mut()
                    .alias_atomic(channel_id.clone().into(), (**id).clone().into())
                {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => true,
                    _ => false,
                }
            }) {
                self.net
                    .add_edge(*channel_node.1, bb_end, PetriNetEdge { label: 1 });
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

        if self.net_config.enable_blocking {
            if let Some(_) = self.handle_lock_call(destination, target, bb_end) {
                log::debug!("callee_func_name with lock: {:?}", callee_func_name);
                return;
            }

            if self.handle_condvar_call(
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

            if callee_func_name.contains("::drop") {
                log::debug!("callee_func_name with drop: {:?}", callee_func_name);
                let lockguard_id = LockGuardId::new(
                    self.instance_id,
                    args.get(0).unwrap().node.place().unwrap().local,
                );
                if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                    let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                    match &self.lockguards[&lockguard_id].lockguard_ty {
                        LockGuardTy::StdMutex(_)
                        | LockGuardTy::ParkingLotMutex(_)
                        | LockGuardTy::SpinMutex(_) => {
                            self.net
                                .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1u8 });

                            match self.net.node_weight_mut(bb_end) {
                                Some(PetriNetNode::T(t)) => {
                                    t.transition_type =
                                        ControlType::Drop(DropType::Unlock(lock_node.clone()));
                                }
                                _ => {}
                            }
                        }

                        LockGuardTy::StdRwLockRead(_)
                        | LockGuardTy::ParkingLotRead(_)
                        | LockGuardTy::SpinRead(_) => {
                            self.net
                                .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1u8 });

                            match self.net.node_weight_mut(bb_end) {
                                Some(PetriNetNode::T(t)) => {
                                    t.transition_type =
                                        ControlType::Drop(DropType::Unlock(lock_node.clone()));
                                }
                                _ => {}
                            }
                        }
                        _ => {
                            self.net
                                .add_edge(bb_end, *lock_node, PetriNetEdge { label: 10u8 });
                            match self.net.node_weight_mut(bb_end) {
                                Some(PetriNetNode::T(t)) => {
                                    t.transition_type =
                                        ControlType::Drop(DropType::Unlock(lock_node.clone()));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                self.connect_to_target(bb_end, target);
                return;
            }

            if self.handle_channel_call(&callee_func_name, args, bb_end, target) {
                log::debug!("callee_func_name with channel: {:?}", callee_func_name);
                return;
            }
        }

        if self.handle_thread_call(&callee_func_name, args, target, bb_end, &bb_idx, span) {
            log::debug!("callee_func_name with thread: {:?}", callee_func_name);
            return;
        }

        if self.net_config.enable_atomic {
            if self.handle_atomic_call(&callee_func_name, args, bb_end, target, &bb_idx, span) {
                log::debug!("callee_func_name with atomic: {:?}", callee_func_name);
                return;
            }
        }

        log::debug!("callee_func_name with normal: {:?}", callee_func_name);
        if callee_func_name.contains("core::panic") {
            self.net
                .add_edge(bb_end, self.entry_exit.1, PetriNetEdge { label: 1 });
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
        let bb_term_transition = Transition::new(bb_term_name, ControlType::Drop(DropType::Basic));
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_vec.get(bb_idx).unwrap().last().unwrap(),
            bb_end,
            PetriNetEdge { label: 1u8 },
        );

        if !bb.is_cleanup && self.net_config.enable_blocking {
            let lockguard_id = LockGuardId::new(self.instance_id, place.local);

            if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                match &self.lockguards[&lockguard_id].lockguard_ty {
                    LockGuardTy::StdMutex(_)
                    | LockGuardTy::ParkingLotMutex(_)
                    | LockGuardTy::SpinMutex(_)
                    | LockGuardTy::StdRwLockRead(_)
                    | LockGuardTy::ParkingLotRead(_)
                    | LockGuardTy::SpinRead(_) => {
                        self.net
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1u8 });
                    }
                    _ => {
                        self.net
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 10u8 });
                    }
                }

                match self.net.node_weight_mut(bb_end) {
                    Some(PetriNetNode::T(t)) => {
                        t.transition_type = ControlType::Drop(DropType::Unlock(lock_node.clone()));
                    }
                    _ => {}
                }
            }
        }

        self.net.add_edge(
            bb_end,
            *self.bb_node_start_end.get(target).unwrap(),
            PetriNetEdge { label: 1u8 },
        );
    }

    fn has_unsafe_alias(&self, place_id: AliasId) -> (bool, NodeIndex, Option<AliasId>) {
        for (unsafe_place, node_index) in self.unsafe_places.iter() {
            match self
                .alias
                .borrow_mut()
                .alias_atomic(place_id.into(), *unsafe_place)
            {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    return (true, node_index.clone(), Some(unsafe_place.clone()));
                }
                _ => return (false, NodeIndex::new(0), None),
            }
        }
        (false, NodeIndex::new(0), None)
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
            Rvalue::Len(place) | Rvalue::Discriminant(place) => {
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
                let read_t = Transition::new(
                    transition_name.clone(),
                    ControlType::UnsafeRead(
                        alias_result.1,
                        span_str.to_string(),
                        bb_idx.index(),
                        place_ty,
                    ),
                );
                let unsafe_read_t = self.net.add_node(PetriNetNode::T(read_t));

                let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
                self.net
                    .add_edge(*bb_nodes, unsafe_read_t, PetriNetEdge { label: 1 });

                let unsafe_place = alias_result.1;
                self.net
                    .add_edge(unsafe_place, unsafe_read_t, PetriNetEdge { label: 1 });
                self.net
                    .add_edge(unsafe_read_t, unsafe_place, PetriNetEdge { label: 1 });

                let place_name = format!("{}_rready", &transition_name.as_str());
                let temp_place = Place::new_with_no_token(place_name, PlaceType::BasicBlock);
                let temp_place_node = self.net.add_node(PetriNetNode::P(temp_place));
                self.net
                    .add_edge(unsafe_read_t, temp_place_node, PetriNetEdge { label: 1 });

                self.bb_node_vec
                    .get_mut(&bb_idx)
                    .unwrap()
                    .push(unsafe_read_t);
                self.bb_node_vec
                    .get_mut(&bb_idx)
                    .unwrap()
                    .push(temp_place_node);
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
            let write_t = Transition::new(
                transition_name.clone(),
                ControlType::UnsafeWrite(
                    alias_result.1,
                    span_str.to_string(),
                    bb_idx.index(),
                    place_ty,
                ),
            );
            let unsafe_write_t = self.net.add_node(PetriNetNode::T(write_t));

            let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
            self.net
                .add_edge(*bb_nodes, unsafe_write_t, PetriNetEdge { label: 1 });

            let unsafe_place = alias_result.1;
            self.net
                .add_edge(unsafe_place, unsafe_write_t, PetriNetEdge { label: 1 });
            self.net
                .add_edge(unsafe_write_t, unsafe_place, PetriNetEdge { label: 1 });

            let place_name = format!("{}_wready", &transition_name.as_str());
            let temp_place = Place::new_with_no_token(place_name, PlaceType::BasicBlock);
            let temp_place_node = self.net.add_node(PetriNetNode::P(temp_place));
            self.net
                .add_edge(unsafe_write_t, temp_place_node, PetriNetEdge { label: 1 });

            self.bb_node_vec
                .get_mut(&bb_idx)
                .unwrap()
                .push(unsafe_write_t);
            self.bb_node_vec
                .get_mut(&bb_idx)
                .unwrap()
                .push(temp_place_node);
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
            log::debug!("Skipping serialization function: {}", fn_name);
            return;
        }

        self.init_basic_block(body, &fn_name);

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                continue;
            }

            if self.net_config.enable_unsafe {
                for stmt in bb.statements.iter() {
                    if let Some(ref term) = bb.terminator {
                        if let TerminatorKind::Assert { .. } = &term.kind {
                            break;
                        }
                    }
                    self.visit_statement_body(stmt, bb_idx);
                }
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
