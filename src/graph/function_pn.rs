use super::{
    callgraph::InstanceId,
    petri_net::{PetriNetEdge, PetriNetNode, Place},
};
use crate::{
    analysis::pointsto::{AliasAnalysis, ApproximateAliasKind},
    concurrency::{
        candvar::CondVarId,
        handler::JoinHanderId,
        locks::{LockGuardId, LockGuardMap, LockGuardTy},
    },
    graph::petri_net::Transition,
};
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{visit::Visitor, BasicBlock, TerminatorKind, UnwindAction};
use rustc_middle::{
    mir::Body,
    ty::{Instance, TyCtxt},
};
use std::{cell::RefCell, collections::HashMap};

pub fn find_key_by_id(
    map: &HashMap<usize, Vec<JoinHanderId>>,
    target_id: JoinHanderId,
) -> Option<usize> {
    for (key, id_vec) in map.iter() {
        if id_vec.iter().any(|&id| id == target_id) {
            return Some(*key);
        }
    }
    None
}

// Constructing Subsequent Graph based on function's CFG
pub struct FunctionPN<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    // param_env: ParamEnv<'tcx>,
    pub net: &'a mut Graph<PetriNetNode, PetriNetEdge>,
    //callgraph: &'b CallGraph<'tcx>,
    alias: &'a RefCell<AliasAnalysis<'b, 'tcx>>,
    pub lockguards: LockGuardMap<'tcx>,
    function_counter: &'a HashMap<DefId, (NodeIndex, NodeIndex)>,
    locks_counter: &'a HashMap<LockGuardId, NodeIndex>,
    bb_node_start_end: HashMap<BasicBlock, NodeIndex>,
    bb_node_vec: HashMap<BasicBlock, Vec<NodeIndex>>,
    thread_id_handler: &'a mut HashMap<usize, Vec<JoinHanderId>>,
    handler_id: &'a mut HashMap<JoinHanderId, DefId>,
    condvar_id: &'a HashMap<CondVarId, NodeIndex>,
}

impl<'a, 'b, 'tcx> FunctionPN<'a, 'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'a Instance<'tcx>,
        body: &'a Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        // param_env: ParamEnv<'tcx>,
        net: &'a mut Graph<PetriNetNode, PetriNetEdge>,
        // callgraph: &'b CallGraph<'tcx>,
        alias: &'a RefCell<AliasAnalysis<'b, 'tcx>>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'a HashMap<DefId, (NodeIndex, NodeIndex)>,
        locks_counter: &'a HashMap<LockGuardId, NodeIndex>,
        thread_id_handler: &'a mut HashMap<usize, Vec<JoinHanderId>>,
        handler_id: &'a mut HashMap<JoinHanderId, DefId>,
        condvar_id: &'a HashMap<CondVarId, NodeIndex>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            // param_env,
            net,
            //    callgraph,
            alias,
            lockguards,
            function_counter,
            locks_counter,
            bb_node_start_end: HashMap::default(),
            bb_node_vec: HashMap::new(),
            thread_id_handler,
            handler_id,
            condvar_id,
        }
    }

    pub fn analyze(&mut self) {
        self.visit_body(self.body);
    }
}

impl<'a, 'b, 'tcx> Visitor<'tcx> for FunctionPN<'a, 'b, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let func_id = self.instance.def_id();

        let fn_name = self.tcx.def_path_str(func_id);
        if fn_name.contains("core")
            || fn_name.contains("std")
            || fn_name.contains("alloc")
            || fn_name.contains("parking_lot::")
            || fn_name.contains("spin::")
            || fn_name.contains("::new")
            || fn_name.contains("libc")
            || fn_name.contains("tokio")
        {
        } else {
            for (bb_idx, _) in body.basic_blocks.iter_enumerated() {
                let bb_name = fn_name.clone() + &format!("{:?}", bb_idx);
                let bb_start_place = Place::new_with_no_token(bb_name);
                let bb_start = self.net.add_node(PetriNetNode::P(bb_start_place));
                self.bb_node_start_end
                    .insert(bb_idx.clone(), bb_start.clone());
                self.bb_node_vec.insert(bb_idx.clone(), vec![bb_start]);
            }
            for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
                // for stmt in &bb.statements {
                //     println!("  statement: {:?}", stmt);
                // }
                if bb_idx.index() == 0 {
                    let bb_start_name = fn_name.clone() + &format!("{:?}", bb_idx) + "start";
                    let bb_start_transition = Transition::new(bb_start_name, (0, 0), 1);
                    let bb_start = self.net.add_node(PetriNetNode::T(bb_start_transition));

                    self.net.add_edge(
                        self.function_counter.get(&func_id).unwrap().0,
                        bb_start,
                        PetriNetEdge { label: 1usize },
                    );
                    self.net.add_edge(
                        bb_start,
                        *self.bb_node_start_end.get(&bb_idx).unwrap(),
                        PetriNetEdge { label: 1usize },
                    );
                }
                if let Some(ref term) = bb.terminator {
                    match &term.kind {
                        TerminatorKind::Goto { target } => {
                            let bb_term_name = fn_name.clone() + &format!("{:?}", bb_idx) + "goto";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );

                            let target_bb_start = self.bb_node_start_end.get(&target).unwrap();
                            self.net.add_edge(
                                bb_end,
                                *target_bb_start,
                                PetriNetEdge { label: 1usize },
                            );
                        }
                        TerminatorKind::SwitchInt { discr: _, targets } => {
                            let mut t_num = 1usize;
                            for t in targets.all_targets() {
                                let bb_term_name = fn_name.clone()
                                    + &format!("{:?}", bb_idx)
                                    + "switch"
                                    + t_num.to_string().as_str();
                                t_num += 1;
                                let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                                let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                                self.net.add_edge(
                                    *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                    bb_end,
                                    PetriNetEdge { label: 1usize },
                                );
                                let target_bb_start = self.bb_node_start_end.get(t).unwrap();
                                self.net.add_edge(
                                    bb_end,
                                    *target_bb_start,
                                    PetriNetEdge { label: 1usize },
                                );
                            }
                        }
                        TerminatorKind::UnwindResume => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "resume";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );
                            let return_node = self.function_counter.get(&func_id).unwrap().1;
                            self.net
                                .add_edge(bb_end, return_node, PetriNetEdge { label: 1usize });
                        }
                        TerminatorKind::UnwindTerminate(_) => {}
                        TerminatorKind::Return => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "return";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );

                            let return_node = self.function_counter.get(&func_id).unwrap().1;
                            self.net
                                .add_edge(bb_end, return_node, PetriNetEdge { label: 1usize });
                        }
                        TerminatorKind::Unreachable => {}
                        TerminatorKind::Assert { target, unwind, .. } => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "assert";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );

                            self.net.add_edge(
                                bb_end,
                                *self.bb_node_start_end.get(target).unwrap(),
                                PetriNetEdge { label: 1usize },
                            );
                            match unwind {
                                UnwindAction::Cleanup(bb_clean) => {
                                    let bb_unwind_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "unwind";
                                    let bb_unwind_transition =
                                        Transition::new(bb_unwind_name, (0, 0), 1);
                                    let bb_unwind =
                                        self.net.add_node(PetriNetNode::T(bb_unwind_transition));
                                    self.net.add_edge(
                                        *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                        bb_unwind,
                                        PetriNetEdge { label: 1usize },
                                    );
                                    self.net.add_edge(
                                        bb_unwind,
                                        *self.bb_node_start_end.get(&bb_clean).unwrap(),
                                        PetriNetEdge { label: 1usize },
                                    );
                                }
                                _ => {}
                            }
                        }
                        TerminatorKind::Call {
                            func,
                            args,
                            destination,
                            target,
                            unwind,
                            call_source: _,
                            fn_span: _,
                        } => {
                            let call_ty = func.ty(self.body, self.tcx).kind();
                            match call_ty {
                                rustc_middle::ty::TyKind::FnDef(_, _)
                                | rustc_middle::ty::TyKind::Closure(_, _) => {}
                                _ => {
                                    return;
                                }
                            }

                            let lockguard_id =
                                LockGuardId::new(self.instance_id, destination.local);
                            let handle_id = JoinHanderId::new(self.instance_id, destination.local);

                            let bb_term_name = fn_name.clone() + &format!("{:?}", bb_idx) + "call";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );

                            if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                                let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                                match &self.lockguards[&lockguard_id].lockguard_ty {
                                    LockGuardTy::StdMutex(_)
                                    | LockGuardTy::ParkingLotMutex(_)
                                    | LockGuardTy::SpinMutex(_)
                                    | LockGuardTy::StdRwLockRead(_)
                                    | LockGuardTy::ParkingLotRead(_)
                                    | LockGuardTy::SpinRead(_) => {
                                        self.net.add_edge(
                                            *lock_node,
                                            bb_end,
                                            PetriNetEdge { label: 1usize },
                                        );
                                    }
                                    _ => {
                                        self.net.add_edge(
                                            *lock_node,
                                            bb_end,
                                            PetriNetEdge { label: 10usize },
                                        );
                                    }
                                }
                                match (target, unwind) {
                                    (Some(return_block), _) => {
                                        self.net.add_edge(
                                            bb_end,
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1usize },
                                        );
                                    }
                                    _ => {}
                                }
                            } else {
                                // link current bb_idx to callee's start place and return, unwind
                                let callee_ty = func.ty(self.body, self.tcx);

                                let callee_id = match callee_ty.kind() {
                                    rustc_middle::ty::TyKind::FnPtr(_) => {
                                        return;
                                    }
                                    rustc_middle::ty::TyKind::FnDef(def_id, _)
                                    | rustc_middle::ty::TyKind::Closure(def_id, _) => {
                                        // println!("callee id: {:?}", *def_id);
                                        *def_id
                                    }
                                    _ => {
                                        panic!("TyKind::FnDef, a function definition, but got: {callee_ty:?}");
                                    }
                                };

                                // 判断Caller是nofity或者wait
                                let caller_func_name = self.tcx.def_path_str(callee_id);
                                if caller_func_name.contains("Condvar::notify_one") {
                                    let condvar_local = args.get(0).unwrap().place().unwrap().local;
                                    let condvar_id =
                                        CondVarId::new(self.instance_id, condvar_local);
                                    println!("condvar nofity: {:?}", condvar_id);
                                    for condvar_e in self.condvar_id.into_iter() {
                                        match self
                                            .alias
                                            .borrow_mut()
                                            .alias_condvar(condvar_id.into(), (*condvar_e.0).into())
                                        {
                                            ApproximateAliasKind::Possibly
                                            | ApproximateAliasKind::Probably => {
                                                // find corresponding condvar
                                                self.net.add_edge(
                                                    bb_end,
                                                    *condvar_e.1,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                match (target, unwind) {
                                                    (Some(return_block), _) => {
                                                        self.net.add_edge(
                                                            bb_end,
                                                            *self
                                                                .bb_node_start_end
                                                                .get(return_block)
                                                                .unwrap(),
                                                            PetriNetEdge { label: 1usize },
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                                return;
                                            }
                                            _ => continue,
                                        }
                                    }
                                } else if caller_func_name.contains("Condvar::wait") {
                                    let bb_wait_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "release lock";
                                    let bb_wait_place = Place::new_with_no_token(bb_wait_name);
                                    let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                                    let bb_ret_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "wait";
                                    let bb_ret_transition = Transition::new(bb_ret_name, (0, 0), 1);
                                    let bb_ret =
                                        self.net.add_node(PetriNetNode::T(bb_ret_transition));

                                    self.net.add_edge(
                                        bb_end,
                                        bb_wait,
                                        PetriNetEdge { label: 1usize },
                                    );
                                    self.net.add_edge(
                                        bb_wait,
                                        bb_ret,
                                        PetriNetEdge { label: 1usize },
                                    );

                                    let condvar_local = args.get(0).unwrap().place().unwrap().local;
                                    let condvar_id =
                                        CondVarId::new(self.instance_id, condvar_local);
                                    println!("condvar wait: {:?}", condvar_id);
                                    for condvar_e in self.condvar_id.into_iter() {
                                        match self
                                            .alias
                                            .borrow_mut()
                                            .alias_condvar(condvar_id.into(), (*condvar_e.0).into())
                                        {
                                            ApproximateAliasKind::Possibly
                                            | ApproximateAliasKind::Probably => {
                                                // find corresponding condvar
                                                self.net.add_edge(
                                                    *condvar_e.1,
                                                    bb_ret,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                            }
                                            _ => continue,
                                        }
                                    }

                                    let condvar_lockguard = LockGuardId::new(
                                        self.instance_id,
                                        args.get(1).unwrap().place().unwrap().local,
                                    );
                                    let condvar_lock_node =
                                        self.locks_counter.get(&condvar_lockguard).unwrap();

                                    self.net.add_edge(
                                        bb_end,
                                        *condvar_lock_node,
                                        PetriNetEdge { label: 1usize },
                                    );
                                    self.net.add_edge(
                                        *condvar_lock_node,
                                        bb_ret,
                                        PetriNetEdge { label: 1usize },
                                    );

                                    match (target, unwind) {
                                        (Some(return_block), _) => {
                                            self.net.add_edge(
                                                bb_ret,
                                                *self.bb_node_start_end.get(return_block).unwrap(),
                                                PetriNetEdge { label: 1usize },
                                            );
                                        }
                                        _ => {}
                                    }
                                } else {
                                    let bb_wait_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "wait";
                                    let bb_wait_place = Place::new_with_no_token(bb_wait_name);
                                    let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                                    let bb_ret_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "return";
                                    let bb_ret_transition = Transition::new(bb_ret_name, (0, 0), 1);
                                    let bb_ret =
                                        self.net.add_node(PetriNetNode::T(bb_ret_transition));

                                    self.net.add_edge(
                                        bb_end,
                                        bb_wait,
                                        PetriNetEdge { label: 1usize },
                                    );
                                    self.net.add_edge(
                                        bb_wait,
                                        bb_ret,
                                        PetriNetEdge { label: 1usize },
                                    );

                                    if args.len() > 0 {
                                        let args_ty = args.get(0).unwrap().ty(self.body, self.tcx);
                                        let _: Option<DefId> = match args_ty.kind() {
                                            rustc_middle::ty::TyKind::Closure(def_id, _) => {
                                                if let Some((callee_start, _)) =
                                                    self.function_counter.get(&def_id)
                                                {
                                                    self.net.add_edge(
                                                        bb_end,
                                                        *callee_start,
                                                        PetriNetEdge { label: 1usize },
                                                    );
                                                }
                                                if let Some(spawn_thread_id) = find_key_by_id(
                                                    &self.thread_id_handler,
                                                    handle_id,
                                                ) {
                                                    if let Some((_, id_vec)) = self
                                                        .thread_id_handler
                                                        .get_key_value(&spawn_thread_id)
                                                    {
                                                        for id in id_vec.iter() {
                                                            self.handler_id
                                                                .insert(id.clone(), def_id.clone());
                                                        }
                                                    }
                                                };
                                                Some(*def_id)
                                            }
                                            rustc_middle::ty::TyKind::Adt(adt_def, _) => {
                                                let path = self.tcx.def_path_str(adt_def.did());
                                                if path.contains("JoinHandle") {
                                                    let move_handle_id = JoinHanderId::new(
                                                        self.instance_id,
                                                        args.get(0).unwrap().place().unwrap().local,
                                                    );
                                                    if let Some(join_id) =
                                                        self.handler_id.get(&move_handle_id)
                                                    {
                                                        if let Some((_, callee_end)) =
                                                            self.function_counter.get(join_id)
                                                        {
                                                            self.net.add_edge(
                                                                *callee_end,
                                                                bb_ret,
                                                                PetriNetEdge { label: 1usize },
                                                            );
                                                        }
                                                    }
                                                }
                                                None
                                            }
                                            _ => None,
                                        };
                                    }

                                    if let Some((
                                        callee_start,
                                        callee_end,
                                        // callee_panic,
                                        // callee_unwind,
                                    )) = self.function_counter.get(&callee_id)
                                    {
                                        self.net.add_edge(
                                            bb_end,
                                            *callee_start,
                                            PetriNetEdge { label: 1usize },
                                        );
                                        match (target, unwind) {
                                            (Some(return_block), _) => {
                                                self.net.add_edge(
                                                    *callee_end,
                                                    bb_ret,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                self.net.add_edge(
                                                    bb_ret,
                                                    *self
                                                        .bb_node_start_end
                                                        .get(return_block)
                                                        .unwrap(),
                                                    PetriNetEdge { label: 1usize },
                                                );
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        match (target, unwind) {
                                            (Some(return_block), _) => {
                                                self.net.add_edge(
                                                    bb_ret,
                                                    *self
                                                        .bb_node_start_end
                                                        .get(return_block)
                                                        .unwrap(),
                                                    PetriNetEdge { label: 1usize },
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        TerminatorKind::Drop {
                            place,
                            target,
                            unwind,
                            replace: _,
                        } => {
                            let bb_term_name = fn_name.clone() + &format!("{:?}", bb_idx) + "drop";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            self.net.add_edge(
                                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1usize },
                            );

                            if !bb.is_cleanup {
                                // bb不检测数据竞争，仅提取操作语义，若Drop MutexGuard跳过

                                let lockguard_id = LockGuardId::new(self.instance_id, place.local);
                                // local is lockguard
                                if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                                    let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                                    match &self.lockguards[&lockguard_id].lockguard_ty {
                                        LockGuardTy::StdMutex(_)
                                        | LockGuardTy::ParkingLotMutex(_)
                                        | LockGuardTy::SpinMutex(_)
                                        | LockGuardTy::StdRwLockRead(_)
                                        | LockGuardTy::ParkingLotRead(_)
                                        | LockGuardTy::SpinRead(_) => {
                                            self.net.add_edge(
                                                bb_end,
                                                *lock_node,
                                                PetriNetEdge { label: 1usize },
                                            );
                                        }
                                        _ => {
                                            self.net.add_edge(
                                                bb_end,
                                                *lock_node,
                                                PetriNetEdge { label: 10usize },
                                            );
                                        }
                                    }
                                }
                            }
                            match (target, unwind) {
                                (
                                    return_block,
                                    UnwindAction::Continue | UnwindAction::Terminate(_),
                                ) => {
                                    self.net.add_edge(
                                        bb_end,
                                        *self.bb_node_start_end.get(return_block).unwrap(),
                                        PetriNetEdge { label: 1usize },
                                    );
                                }

                                (return_block, UnwindAction::Cleanup(bb_clean)) => {
                                    self.net.add_edge(
                                        bb_end,
                                        *self.bb_node_start_end.get(return_block).unwrap(),
                                        PetriNetEdge { label: 1usize },
                                    );
                                    let bb_unwind_name =
                                        fn_name.clone() + &format!("{:?}", bb_idx) + "unwind";
                                    let bb_unwind_transition =
                                        Transition::new(bb_unwind_name, (0, 0), 1);
                                    let bb_unwind =
                                        self.net.add_node(PetriNetNode::T(bb_unwind_transition));
                                    self.net.add_edge(
                                        *self.bb_node_start_end.get(&bb_idx).unwrap(),
                                        bb_unwind,
                                        PetriNetEdge { label: 1usize },
                                    );
                                    self.net.add_edge(
                                        bb_unwind,
                                        *self.bb_node_start_end.get(&bb_clean).unwrap(),
                                        PetriNetEdge { label: 1usize },
                                    );
                                }

                                _ => {}
                            }
                        }
                        TerminatorKind::Yield { .. } => {
                            unimplemented!("TerminatorKind::Yield not implemented yet")
                        }
                        // TerminatorKind::CoroutineDrop => {
                        //     unimplemented!("TerminatorKind::GeneratorDrop not implemented yet")
                        // }
                        TerminatorKind::FalseEdge { .. } => {
                            unimplemented!("TerminatorKind::FalseEdge not implemented yet")
                        }
                        TerminatorKind::FalseUnwind { .. } => {
                            unimplemented!("TerminatorKind::FalseUnwind not implemented yet")
                        }
                        TerminatorKind::InlineAsm { .. } => {
                            unimplemented!("TerminatorKind::InlineAsm not implemented yet")
                        }
                        _ => {}
                    }
                    // println!("  terminator: {:?}", term);
                }
            }
        }
    }
}
