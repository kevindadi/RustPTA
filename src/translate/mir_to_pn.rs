use super::callgraph::{CallGraph, InstanceId};
use crate::{
    concurrency::{
        atomic::AtomicOrdering,
        blocking::{CondVarId, LockGuardId, LockGuardMap, LockGuardTy},
        channel::ChannelId,
    },
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    util::format_name,
};
use crate::{
    net::{
        structure::PlaceType, Idx, Net, Place, PlaceId, Transition, TransitionId, TransitionType,
    },
    translate::key_api::KeyApiRegex,
};
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
    pub net: &'translate mut Net,
    alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
    pub lockguards: LockGuardMap<'tcx>,
    function_counter: &'translate HashMap<DefId, (PlaceId, PlaceId)>,
    locks_counter: &'translate HashMap<LockGuardId, PlaceId>,
    bb_node_start_end: HashMap<BasicBlock, PlaceId>,
    bb_node_vec: HashMap<BasicBlock, Vec<PlaceId>>,
    condvar_id: &'translate HashMap<CondVarId, PlaceId>,
    atomic_places: &'translate HashMap<AliasId, PlaceId>,
    atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
    pub exclude_bb: HashSet<usize>,
    return_transition: TransitionId,
    entry_exit: (PlaceId, PlaceId),
    unsafe_places: &'translate HashMap<AliasId, PlaceId>,
    key_api_regex: &'translate KeyApiRegex,
    channel_places: &'translate HashMap<ChannelId, PlaceId>,
}

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        callgraph: &'translate CallGraph<'tcx>,
        net: &'translate mut Net,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'translate HashMap<DefId, (PlaceId, PlaceId)>,
        locks_counter: &'translate HashMap<LockGuardId, PlaceId>,
        condvar_id: &'translate HashMap<CondVarId, PlaceId>,
        atomic_places: &'translate HashMap<AliasId, PlaceId>,
        atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
        entry_exit: (PlaceId, PlaceId),
        unsafe_places: &'translate HashMap<AliasId, PlaceId>,
        key_api_regex: &'translate KeyApiRegex,
        channel_places: &'translate HashMap<ChannelId, PlaceId>,
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
            return_transition: TransitionId::new(0),
            entry_exit,
            unsafe_places,
            key_api_regex,
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
            let bb_start_place = Place::new(bb_name, 0, 1, PlaceType::BasicBlock, bb_span);
            let bb_start = self.net.add_place(bb_start_place);
            self.bb_node_start_end
                .insert(bb_idx.clone(), bb_start.clone());
            self.bb_node_vec.insert(bb_idx.clone(), vec![bb_start]);
        }
    }

    fn handle_start_block(&mut self, name: &str, bb_idx: BasicBlock, def_id: DefId) {
        let bb_start_name = format!("{}_{}_start", name, bb_idx.index());
        let bb_start_transition = Transition::new_with_transition_type(
            bb_start_name,
            TransitionType::Start(self.instance_id.index()),
        );
        let bb_start = self.net.add_transition(bb_start_transition);

        self.net
            .add_output_arc(self.function_counter.get(&def_id).unwrap().0, bb_start, 1);
        self.net
            .add_input_arc(*self.bb_node_start_end.get(&bb_idx).unwrap(), bb_start, 1);
    }

    fn handle_assert(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "assert");
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Assert);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            1,
        );
        self.net
            .add_input_arc(*self.bb_node_start_end.get(target).unwrap(), bb_end, 1);
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
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Goto);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            1,
        );

        let target_bb_start = self.bb_node_start_end.get(&target).unwrap();
        self.net.add_input_arc(*target_bb_start, bb_end, 1);
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

            self.net.add_output_arc(
                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                bb_end,
                1,
            );
            let target_bb_start = self.bb_node_start_end.get(t).unwrap();
            self.net.add_input_arc(*target_bb_start, bb_end, 1);
        }
    }

    fn handle_return(&mut self, bb_idx: BasicBlock, name: &str) {
        let return_node = self
            .function_counter
            .get(&self.instance.def_id())
            .unwrap()
            .1;

        if self.return_transition.raw() == 0 {
            let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_term_transition = Transition::new_with_transition_type(
                bb_term_name,
                TransitionType::Return(self.instance_id.index()),
            );
            let bb_end = self.net.add_transition(bb_term_transition);

            self.return_transition = bb_end.clone();
        }

        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            self.return_transition,
            1,
        );
        self.net
            .add_input_arc(return_node, self.return_transition, 1);
    }

    fn create_call_transition(&mut self, bb_idx: BasicBlock, bb_term_name: &str) -> TransitionId {
        let bb_term_transition = Transition::new_with_transition_type(
            bb_term_name.to_string(),
            TransitionType::Function,
        );
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_end,
            1,
        );
        bb_end
    }

    fn handle_lock_call(
        &mut self,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
    ) -> Option<TransitionType> {
        let lockguard_id = LockGuardId::new(self.instance_id, destination.local);
        if let Some(guard) = self.lockguards.get_mut(&lockguard_id) {
            let lock_node = self.locks_counter.get(&lockguard_id).unwrap();

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
        self.net.add_output_arc(*lock_node, bb_end, 1);
    }

    fn connect_to_target(&mut self, bb_end: TransitionId, target: &Option<BasicBlock>) {
        if let Some(target_bb) = target {
            self.net
                .add_input_arc(*self.bb_node_start_end.get(target_bb).unwrap(), bb_end, 1);
        }
    }

    fn handle_thread_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: TransitionId,
        bb_idx: &BasicBlock,
        span: &str,
    ) -> bool {
        if self.key_api_regex.thread_spawn.is_match(callee_func_name) {
            self.handle_spawn(callee_func_name, args, target, bb_end);
            true
        } else if self.key_api_regex.scope_join.is_match(callee_func_name) {
            self.handle_scope_join(callee_func_name, args, target, bb_end);
            true
        } else if self.key_api_regex.thread_join.is_match(callee_func_name) {
            self.handle_join(callee_func_name, args, target, bb_end);
            true
        } else if callee_func_name.contains("rayon_core::join") {
            self.handle_rayon_join(callee_func_name, bb_idx, args, target, bb_end, span);
            true
        } else if self.key_api_regex.scope_spwan.is_match(callee_func_name) {
            self.handle_scope_spawn(callee_func_name, bb_idx, args, target, bb_end);
            true
        } else {
            false
        }
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

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            self.net.add_output_arc(
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
                bb_end,
                1,
            );
            self.net.add_input_arc(
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
                bb_end,
                1,
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
                        self.net.add_input_arc(
                            self.function_counter.get(&closure_def_id).unwrap().0,
                            bb_end,
                            1,
                        );
                        self.net.add_input_arc(
                            self.function_counter.get(&closure_def_id).unwrap().1,
                            self.return_transition,
                            1,
                        );
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            self.net.add_input_arc(
                                self.function_counter.get(&closure_def_id).unwrap().0,
                                bb_end,
                                1,
                            );
                            self.net.add_output_arc(
                                self.function_counter.get(&closure_def_id).unwrap().1,
                                self.return_transition,
                                1,
                            );
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                self.net.add_input_arc(
                                    self.function_counter.get(closure_def_id).unwrap().0,
                                    bb_end,
                                    1,
                                );
                                self.net.add_output_arc(
                                    self.function_counter.get(&closure_def_id).unwrap().1,
                                    self.return_transition,
                                    1,
                                );
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

        self.net.add_input_arc(bb_wait, bb_end, 1);
        self.net.add_output_arc(bb_wait, bb_join, 1);

        self.connect_to_target(bb_join, target);

        for arg in args {
            if let Operand::Move(place) | Operand::Copy(place) = &arg.node {
                let place_ty = place.ty(self.body, self.tcx).ty;
                if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                    place_ty.kind()
                {
                    self.net.add_input_arc(
                        self.function_counter.get(&closure_def_id).unwrap().0,
                        bb_end,
                        1,
                    );

                    self.net.add_output_arc(
                        self.function_counter.get(&closure_def_id).unwrap().1,
                        bb_join,
                        1,
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
        bb_end: TransitionId,
    ) {
        if let Some(closure_arg) = args.first() {
            match &closure_arg.node {
                Operand::Move(place) | Operand::Copy(place) => {
                    let place_ty = place.ty(self.body, self.tcx).ty;
                    if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                        place_ty.kind()
                    {
                        self.net.add_input_arc(
                            self.function_counter.get(&closure_def_id).unwrap().0,
                            bb_end,
                            1,
                        );
                    }
                }
                Operand::Constant(constant) => {
                    let const_val = constant.const_;
                    match const_val {
                        Const::Unevaluated(unevaluated, _) => {
                            let closure_def_id = unevaluated.def;
                            self.net.add_input_arc(
                                self.function_counter.get(&closure_def_id).unwrap().0,
                                bb_end,
                                1,
                            );
                        }
                        _ => {
                            if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                                constant.ty().kind()
                            {
                                self.net.add_input_arc(
                                    self.function_counter.get(closure_def_id).unwrap().0,
                                    bb_end,
                                    1,
                                );
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

            if let Some(transition) = self.net.get_transition_mut(bb_end) {
                transition.transition_type = TransitionType::Join(callee_func_name.to_string());
            }

            self.net.add_output_arc(
                self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
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
        if let Some((callee_start, callee_end)) = self.function_counter.get(callee_id) {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new(bb_wait_name, 0, 1, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_place(bb_wait_place);

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
            let bb_ret_transition =
                Transition::new_with_transition_type(bb_ret_name, TransitionType::Function);
            let bb_ret = self.net.add_transition(bb_ret_transition);

            self.net.add_input_arc(bb_wait, bb_end, 1);
            self.net.add_output_arc(bb_wait, bb_ret, 1);
            self.net.add_input_arc(*callee_start, bb_end, 1);
            match target {
                Some(return_block) => {
                    self.net.add_output_arc(*callee_end, bb_ret, 1);
                    self.net.add_input_arc(
                        *self.bb_node_start_end.get(return_block).unwrap(),
                        bb_ret,
                        1,
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

                                self.net.add_input_arc(bb_wait, bb_end, 1);
                                self.net.add_output_arc(bb_wait, bb_ret, 1);
                                self.net.add_input_arc(*callee_start, bb_end, 1);
                                match target {
                                    Some(return_block) => {
                                        self.net.add_output_arc(*callee_end, bb_ret, 1);
                                        self.net.add_input_arc(
                                            *self.bb_node_start_end.get(return_block).unwrap(),
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
        bb_end: TransitionId,
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

            let atomic_load_place = Place::new(
                format!(
                    "atomic_load_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                1,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_load_place_node = self.net.add_place(atomic_load_place);
            self.net.add_input_arc(atomic_load_place_node, bb_end, 1);

            if let Some(order) = self.atomic_order_maps.get(&current_id) {
                let atomic_load_transition = Transition::new_with_transition_type(
                    format!(
                        "atomic_{:?}_load_{:?}_{:?}",
                        self.instance_id.index(),
                        order,
                        bb_idx.index()
                    ),
                    TransitionType::AtomicLoad(
                        atomic_e.0.clone().into(),
                        order.clone(),
                        span.to_string(),
                        self.instance_id.index(),
                    ),
                );
                let atomic_load_transition_node = self.net.add_transition(atomic_load_transition);

                self.net
                    .add_output_arc(atomic_load_place_node, atomic_load_transition_node, 1);
                self.net
                    .add_input_arc(*atomic_e.1, atomic_load_transition_node, 1);
                self.net
                    .add_output_arc(*atomic_e.1, atomic_load_transition_node, 1);

                if let Some(t) = target {
                    self.net.add_input_arc(
                        *self.bb_node_start_end.get(t).unwrap(),
                        atomic_load_transition_node,
                        1,
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
        bb_end: TransitionId,
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

            let atomic_store_place = Place::new(
                format!(
                    "atomic_store_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                1,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_store_place_node = self.net.add_place(atomic_store_place);
            self.net.add_input_arc(atomic_store_place_node, bb_end, 1);

            if let Some(order) = self.atomic_order_maps.get(&current_id) {
                let atomic_store_transition = Transition::new_with_transition_type(
                    format!(
                        "atomic_{:?}_store_{:?}_{:?}",
                        self.instance_id.index(),
                        order,
                        bb_idx.index()
                    ),
                    TransitionType::AtomicStore(
                        atomic_e.0.clone().into(),
                        order.clone(),
                        span.to_string(),
                        self.instance_id.index(),
                    ),
                );
                let atomic_store_transition_node = self.net.add_transition(atomic_store_transition);

                self.net
                    .add_output_arc(atomic_store_place_node, atomic_store_transition_node, 1);
                self.net
                    .add_input_arc(*atomic_e.1, atomic_store_transition_node, 1);
                self.net
                    .add_output_arc(*atomic_e.1, atomic_store_transition_node, 1);

                if let Some(t) = target {
                    self.net.add_input_arc(
                        *self.bb_node_start_end.get(t).unwrap(),
                        atomic_store_transition_node,
                        1,
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
        bb_end: TransitionId,
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

            let atomic_cmpxchg_place = Place::new(
                format!(
                    "atomic_cmpxchg_in_{:?}_{:?}",
                    current_id.instance_id.index(),
                    bb_idx.index()
                ),
                0,
                1,
                PlaceType::BasicBlock,
                span.to_string(),
            );
            let atomic_cmpxchg_place_node = self.net.add_place(atomic_cmpxchg_place);
            self.net.add_input_arc(atomic_cmpxchg_place_node, bb_end, 1);

            if let (Some(success_order), Some(failure_order)) = (
                self.atomic_order_maps.get(&current_id),
                self.atomic_order_maps.get(&AliasId::new(
                    self.instance_id,
                    args.get(1).unwrap().node.place().unwrap().local,
                )),
            ) {
                let atomic_cmpxchg_transition = Transition::new_with_transition_type(
                    format!(
                        "atomic_{:?}_cmpxchg_{:?}_{:?}",
                        self.instance_id.index(),
                        success_order,
                        bb_idx.index()
                    ),
                    TransitionType::AtomicCmpXchg(
                        atomic_e.0.clone().into(),
                        success_order.clone(),
                        failure_order.clone(),
                        span.to_string(),
                        self.instance_id.index(),
                    ),
                );
                let atomic_cmpxchg_transition_node =
                    self.net.add_transition(atomic_cmpxchg_transition);

                self.net.add_output_arc(
                    atomic_cmpxchg_place_node,
                    atomic_cmpxchg_transition_node,
                    1,
                );
                self.net
                    .add_input_arc(*atomic_e.1, atomic_cmpxchg_transition_node, 1);
                self.net
                    .add_output_arc(*atomic_e.1, atomic_cmpxchg_transition_node, 1);

                if let Some(t) = target {
                    self.net.add_input_arc(
                        *self.bb_node_start_end.get(t).unwrap(),
                        atomic_cmpxchg_transition_node,
                        1,
                    );
                }
            }
            return true;
        }
        false
    }

    fn handle_condvar_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
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
                        self.net.add_input_arc(*node, bb_end, 1);

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
        } else if self.key_api_regex.condvar_wait.is_match(callee_func_name) {
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new(bb_wait_name, 0, 1, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_place(bb_wait_place);

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "ret");
            let bb_ret_transition =
                Transition::new_with_transition_type(bb_ret_name, TransitionType::Wait);
            let bb_ret = self.net.add_transition(bb_ret_transition);

            self.net.add_input_arc(bb_wait, bb_end, 1);
            self.net.add_output_arc(bb_wait, bb_ret, 1);

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
                        self.net.add_output_arc(*node, bb_ret, 1);
                    }
                    _ => continue,
                }
            }

            let guard_id = LockGuardId::new(
                self.instance_id,
                args.get(1).unwrap().node.place().unwrap().local,
            );
            let lock_node = self.locks_counter.get(&guard_id).unwrap();
            self.net.add_input_arc(*lock_node, bb_end, 1);
            self.net.add_output_arc(*lock_node, bb_ret, 1);

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
        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_term_node,
            1,
        );
        self.net.add_input_arc(self.entry_exit.1, bb_term_node, 1);
    }

    fn handle_panic(&mut self, bb_idx: BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "panic");
        let bb_term_transition = Transition::new_with_transition_type(
            bb_term_name,
            TransitionType::Return(self.instance_id.index()),
        );
        let bb_term_node = self.net.add_transition(bb_term_transition);
        self.net.add_output_arc(
            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
            bb_term_node,
            1,
        );
        self.net.add_input_arc(self.entry_exit.1, bb_term_node, 1);
    }

    fn handle_channel_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        bb_end: TransitionId,
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
                self.net.add_input_arc(*channel_node.1, bb_end, 1);
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
                self.net.add_output_arc(*channel_node.1, bb_end, 1);
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

        if self.handle_condvar_call(&callee_func_name, args, bb_end, target, name, &bb_idx, span) {
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
                            .add_input_arc(*lock_node, bb_end, 1);

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
                        self.net
                            .add_input_arc(*lock_node, bb_end, 1);

                        match self.net.get_transition_mut(bb_end) {
                            Some(transition) => {
                                transition.transition_type =
                                    TransitionType::Unlock(lock_node.index());
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        self.net
                            .add_input_arc(*lock_node, bb_end, 1);
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

        if self.handle_channel_call(&callee_func_name, args, bb_end, target) {
            log::debug!("callee_func_name with channel: {:?}", callee_func_name);
            return;
        }

        if self.handle_thread_call(&callee_func_name, args, target, bb_end, &bb_idx, span) {
            log::debug!("callee_func_name with thread: {:?}", callee_func_name);
            return;
        }

       
        if self.handle_atomic_call(&callee_func_name, args, bb_end, target, &bb_idx, span) {
            log::debug!("callee_func_name with atomic: {:?}", callee_func_name);
            return;
        }
        

        log::debug!("callee_func_name with normal: {:?}", callee_func_name);
        if callee_func_name.contains("core::panic") {
            self.net
                .add_output_arc(self.entry_exit.1, bb_end, 1);
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
        let bb_term_transition = Transition::new_with_transition_type(bb_term_name, TransitionType::Drop);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net.add_output_arc(*self.bb_node_vec.get(bb_idx).unwrap().last().unwrap(), bb_end, 1);

        if !bb.is_cleanup {
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
                            .add_input_arc(*lock_node, bb_end, 1);
                    }
                    _ => {
                        self.net
                            .add_input_arc(*lock_node, bb_end, 1);
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

        self.net.add_input_arc(*self.bb_node_start_end.get(target).unwrap(), bb_end, 1);
    }

    fn has_unsafe_alias(&self, place_id: AliasId) -> (bool, PlaceId, Option<AliasId>) {
        for (unsafe_place, node_index) in self.unsafe_places.iter() {
            match self
                .alias
                .borrow_mut()
                .alias_atomic(place_id.into(), *unsafe_place)
            {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    return (true, node_index.clone(), Some(unsafe_place.clone()));
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
                let read_t = Transition::new_with_transition_type(
                    transition_name.clone(),
                    TransitionType::UnsafeRead(alias_result.1.index(), span_str.to_string(), bb_idx.index(), place_ty),
                );
                let unsafe_read_t = self.net.add_transition(read_t);

                let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
                self.net
                    .add_output_arc(*bb_nodes, unsafe_read_t, 1);

                let unsafe_place = alias_result.1;
                self.net
                    .add_output_arc(unsafe_place, unsafe_read_t, 1);
                self.net
                    .add_input_arc(unsafe_place, unsafe_read_t, 1);

                let place_name = format!("{}_rready", &transition_name.as_str());
                let temp_place = Place::new(place_name, 0, 1, PlaceType::BasicBlock, span_str.to_string());
                let temp_place_node = self.net.add_place(temp_place);
                self.net
                    .add_input_arc(temp_place_node, unsafe_read_t, 1);

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
            let write_t = Transition::new_with_transition_type(
                transition_name.clone(),
                TransitionType::UnsafeWrite(alias_result.1.index(), span_str.to_string(), bb_idx.index(), place_ty),
            );
            let unsafe_write_t = self.net.add_transition(write_t);

            let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
            self.net
                .add_output_arc(*bb_nodes, unsafe_write_t, 1);

            let unsafe_place = alias_result.1;
            self.net
                .add_output_arc(unsafe_place, unsafe_write_t, 1);
            self.net
                .add_input_arc(unsafe_place, unsafe_write_t, 1);

            let place_name = format!("{}_wready", &transition_name.as_str());
            let temp_place = Place::new(place_name, 0, 1, PlaceType::BasicBlock, span_str.to_string());
            let temp_place_node = self.net.add_place(temp_place);
            self.net
                .add_input_arc(temp_place_node, unsafe_write_t, 1);

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
