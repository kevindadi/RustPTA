use super::{
    callgraph::InstanceId,
    petri_net::{PetriNetEdge, PetriNetNode, Place},
};
use crate::{
    concurrency::locks::{LockGuardId, LockGuardMap, LockGuardTy},
    graph::petri_net::{PetriNet, Transition},
};
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    visit::Visitor, BasicBlock, TerminatorKind, UnwindAction, UnwindTerminateReason,
};
use rustc_middle::{
    mir::Body,
    ty::{Instance, ParamEnv, TyCtxt},
};
use std::collections::HashMap;

// Constructing Subsequent Graph based on function's CFG
pub struct FunctionPN<'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'b Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ParamEnv<'tcx>,
    pub net: &'b mut Graph<PetriNetNode, PetriNetEdge>,
    pub lockguards: LockGuardMap<'tcx>,
    function_counter: &'b HashMap<DefId, (NodeIndex, NodeIndex)>,
    locks_counter: &'b HashMap<LockGuardId, NodeIndex>,
    bb_node_start_end: HashMap<BasicBlock, NodeIndex>,
    bb_node_vec: HashMap<BasicBlock, Vec<NodeIndex>>,
    program_panic: NodeIndex,
}

impl<'b, 'tcx> FunctionPN<'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'b Instance<'tcx>,
        body: &'b Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: ParamEnv<'tcx>,
        net: &'b mut Graph<PetriNetNode, PetriNetEdge>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'b HashMap<DefId, (NodeIndex, NodeIndex)>,
        locks_counter: &'b HashMap<LockGuardId, NodeIndex>,
        program_panic: NodeIndex,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            param_env,
            net,
            lockguards,
            function_counter,
            locks_counter,
            bb_node_start_end: HashMap::default(),
            bb_node_vec: HashMap::new(),
            program_panic,
        }
    }

    pub fn analyze(&mut self) {
        self.visit_body(self.body);
    }
}

impl<'b, 'tcx> Visitor<'tcx> for FunctionPN<'b, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let fn_id = self.instance.def_id();

        let fn_name = self.tcx.def_path_str(fn_id);
        if fn_name.contains("core")
            || fn_name.contains("std")
            || fn_name.contains("alloc")
            || fn_name.contains("parking_lot::")
            || fn_name.contains("spin::")
            || fn_name.contains("::new")
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
                        self.function_counter.get(&fn_id).unwrap().0,
                        bb_start,
                        PetriNetEdge { label: 1u32 },
                    );
                    self.net.add_edge(
                        bb_start,
                        *self.bb_node_start_end.get(&bb_idx).unwrap(),
                        PetriNetEdge { label: 1u32 },
                    );
                }
                if let Some(ref term) = bb.terminator {
                    match &term.kind {
                        TerminatorKind::Goto { target } => {
                            let bb_term_name = fn_name.clone() + &format!("{:?}", bb_idx) + "goto";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
                            let target_bb_start = self.bb_node_start_end.get(&target).unwrap();
                            self.net.add_edge(
                                bb_end,
                                *target_bb_start,
                                PetriNetEdge { label: 1u32 },
                            );
                        }
                        TerminatorKind::SwitchInt { discr: _, targets } => {
                            let mut t_num = 1u32;
                            for t in targets.all_targets() {
                                let bb_term_name = fn_name.clone()
                                    + &format!("{:?}", bb_idx)
                                    + "switch"
                                    + t_num.to_string().as_str();
                                t_num += 1;
                                let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                                let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                                self.net.add_edge(
                                    *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                    bb_end,
                                    PetriNetEdge { label: 1u32 },
                                );
                                let target_bb_start = self.bb_node_start_end.get(t).unwrap();
                                self.net.add_edge(
                                    bb_end,
                                    *target_bb_start,
                                    PetriNetEdge { label: 1u32 },
                                );
                            }
                        }
                        TerminatorKind::UnwindResume => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "UnwindResume";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
                            // let resume_node = self.function_counter.get(&fn_id).unwrap().2;
                            self.net.add_edge(
                                bb_end,
                                self.program_panic,
                                PetriNetEdge { label: 1u32 },
                            );
                        }
                        TerminatorKind::UnwindTerminate(_) => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "UnwindTerminate";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
                            // let panic_node = self.function_counter.get(&fn_id).unwrap().2;
                            self.net.add_edge(
                                bb_end,
                                self.program_panic,
                                PetriNetEdge { label: 1u32 },
                            );
                        }
                        TerminatorKind::Return => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "return";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
                            let return_node = self.function_counter.get(&fn_id).unwrap().1;
                            self.net
                                .add_edge(bb_end, return_node, PetriNetEdge { label: 1u32 });
                        }
                        TerminatorKind::Unreachable => {}
                        TerminatorKind::Assert { target, unwind, .. } => {
                            let bb_term_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "assert";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            let bb_unwind_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "unwind";
                            let bb_unwind_transition = Transition::new(bb_unwind_name, (0, 0), 1);
                            let bb_unwind =
                                self.net.add_node(PetriNetNode::T(bb_unwind_transition));

                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_unwind,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.net.add_edge(
                                bb_end,
                                *self.bb_node_start_end.get(target).unwrap(),
                                PetriNetEdge { label: 1u32 },
                            );
                            match unwind {
                                UnwindAction::Continue | UnwindAction::Terminate(_) => {
                                    self.net.add_edge(
                                        bb_unwind,
                                        self.program_panic,
                                        PetriNetEdge { label: 1u32 },
                                    );
                                }
                                // UnwindAction::Terminate(_) => {
                                //     self.net.add_edge(
                                //         bb_unwind,
                                //         self.function_counter.get(&fn_id).unwrap().2,
                                //         PetriNetEdge { label: 1u32 },
                                //     );
                                // }
                                UnwindAction::Cleanup(bb_clean) => {
                                    self.net.add_edge(
                                        bb_unwind,
                                        *self.bb_node_start_end.get(&bb_clean).unwrap(),
                                        PetriNetEdge { label: 1u32 },
                                    );
                                }

                                _ => {}
                            }
                        }
                        TerminatorKind::Call {
                            func,
                            args: _,
                            destination,
                            target,
                            unwind,
                            call_source: _,
                            fn_span: _,
                        } => {
                            let lockguard_id =
                                LockGuardId::new(self.instance_id, destination.local);

                            let bb_term_name = fn_name.clone() + &format!("{:?}", bb_idx) + "call";
                            let bb_term_transition = Transition::new(bb_term_name, (0, 0), 1);
                            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                            let bb_wait_name = fn_name.clone() + &format!("{:?}", bb_idx) + "wait";
                            let bb_wait_place = Place::new_with_no_token(bb_wait_name);
                            let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                            let bb_ret_name = fn_name.clone() + &format!("{:?}", bb_idx) + "return";
                            let bb_ret_transition = Transition::new(bb_ret_name, (0, 0), 1);
                            let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

                            let bb_unwind_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "unwind";
                            let bb_unwind_transition = Transition::new(bb_unwind_name, (0, 0), 1);
                            let bb_unwind =
                                self.net.add_node(PetriNetNode::T(bb_unwind_transition));

                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );

                            self.net
                                .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1u32 });
                            self.net
                                .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1u32 });
                            self.net
                                .add_edge(bb_wait, bb_unwind, PetriNetEdge { label: 1u32 });

                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
                            if let Some(info) = self.lockguards.get_mut(&lockguard_id) {
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
                                            bb_ret,
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    _ => {
                                        self.net.add_edge(
                                            *lock_node,
                                            bb_ret,
                                            PetriNetEdge { label: 10u32 },
                                        );
                                    }
                                }
                                match (target, unwind) {
                                    (Some(return_block), UnwindAction::Continue) => {
                                        self.net.add_edge(
                                            bb_ret,
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1u32 },
                                        );
                                        self.net.add_edge(
                                            bb_unwind,
                                            self.program_panic,
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    (Some(return_block), UnwindAction::Terminate(_)) => {
                                        self.net.add_edge(
                                            bb_ret,
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1u32 },
                                        );
                                        self.net.add_edge(
                                            bb_unwind,
                                            self.program_panic,
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    (Some(return_block), UnwindAction::Cleanup(bb_clean)) => {
                                        self.net.add_edge(
                                            bb_ret,
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1u32 },
                                        );
                                        self.net.add_edge(
                                            bb_unwind,
                                            *self.bb_node_start_end.get(bb_clean).unwrap(),
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    (None, UnwindAction::Continue) => {
                                        self.net.add_edge(
                                            bb_unwind,
                                            self.program_panic,
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    (None, UnwindAction::Terminate(_)) => {
                                        self.net.add_edge(
                                            bb_unwind,
                                            self.program_panic,
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    (None, UnwindAction::Cleanup(bb_clean)) => {
                                        self.net.add_edge(
                                            bb_unwind,
                                            *self.bb_node_start_end.get(bb_clean).unwrap(),
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    _ => {}
                                }
                            } else {
                                // link current bb_idx to callee's start place and return, unwind
                                let callee_ty = func.ty(self.body, self.tcx);

                                let callee_id = match callee_ty.kind() {
                                    rustc_middle::ty::TyKind::FnPtr(_) => {
                                        unimplemented!(
                                        "TyKind::FnPtr not implemented yet. Function pointers are present in the MIR"
                                    );
                                    }
                                    rustc_middle::ty::TyKind::FnDef(def_id, _)
                                    | rustc_middle::ty::TyKind::Closure(def_id, _) => *def_id,
                                    _ => {
                                        panic!("TyKind::FnDef, a function definition, but got: {callee_ty:?}");
                                    }
                                };

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
                                        PetriNetEdge { label: 1u32 },
                                    );
                                    match (target, unwind) {
                                        (
                                            Some(return_block),
                                            UnwindAction::Continue | UnwindAction::Terminate(_),
                                        ) => {
                                            self.net.add_edge(
                                                *callee_end,
                                                bb_ret,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            self.net.add_edge(
                                                bb_ret,
                                                *self.bb_node_start_end.get(return_block).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            self.net.add_edge(
                                                bb_unwind,
                                                self.program_panic,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        // (Some(return_block), UnwindAction::Terminate(_)) => {
                                        //     self.net.add_edge(
                                        //         *callee_end,
                                        //         *self.bb_node_start_end.get(return_block).unwrap(),
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        //     self.net.add_edge(
                                        //         bb_ret,
                                        //         *self.bb_node_start_end.get(return_block).unwrap(),
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        //     self.net.add_edge(
                                        //         *callee_panic,
                                        //         bb_unwind,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        //     self.net.add_edge(
                                        //         bb_unwind,
                                        //         self.function_counter.get(&fn_id).unwrap().2,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        // }
                                        (Some(return_block), UnwindAction::Cleanup(bb_clean)) => {
                                            self.net.add_edge(
                                                *callee_end,
                                                bb_ret,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            self.net.add_edge(
                                                bb_ret,
                                                *self.bb_node_start_end.get(return_block).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            // self.net.add_edge(
                                            //     *callee_unwind,
                                            //     bb_unwind,
                                            //     PetriNetEdge { label: 1u32 },
                                            // );
                                            self.net.add_edge(
                                                bb_unwind,
                                                *self.bb_node_start_end.get(bb_clean).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        (
                                            None,
                                            UnwindAction::Continue | UnwindAction::Terminate(_),
                                        ) => {
                                            self.net.add_edge(
                                                bb_unwind,
                                                self.program_panic,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        // (None, UnwindAction::Terminate(_)) => {
                                        //     self.net.add_edge(
                                        //         *callee_panic,
                                        //         bb_unwind,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        //     self.net.add_edge(
                                        //         bb_unwind,
                                        //         self.function_counter.get(&fn_id).unwrap().2,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        // }
                                        (None, UnwindAction::Cleanup(bb_clean)) => {
                                            self.net.add_edge(
                                                bb_unwind,
                                                *self.bb_node_start_end.get(bb_clean).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        _ => {}
                                    }
                                } else {
                                    match (target, unwind) {
                                        (
                                            Some(return_block),
                                            UnwindAction::Continue | UnwindAction::Terminate(_),
                                        ) => {
                                            self.net.add_edge(
                                                bb_ret,
                                                *self.bb_node_start_end.get(return_block).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            self.net.add_edge(
                                                bb_unwind,
                                                self.program_panic,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        // (Some(return_block), UnwindAction::Terminate(_)) => {
                                        //     self.net.add_edge(
                                        //         bb_ret,
                                        //         *self.bb_node_start_end.get(return_block).unwrap(),
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        //     self.net.add_edge(
                                        //         bb_unwind,
                                        //         self.function_counter.get(&fn_id).unwrap().2,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        // }
                                        (Some(return_block), UnwindAction::Cleanup(bb_clean)) => {
                                            self.net.add_edge(
                                                bb_ret,
                                                *self.bb_node_start_end.get(return_block).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                            self.net.add_edge(
                                                bb_unwind,
                                                *self.bb_node_start_end.get(bb_clean).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        (
                                            None,
                                            UnwindAction::Continue | UnwindAction::Terminate(_),
                                        ) => {
                                            self.net.add_edge(
                                                bb_unwind,
                                                self.program_panic,
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        // (None, UnwindAction::Terminate(_)) => {
                                        //     self.net.add_edge(
                                        //         bb_unwind,
                                        //         self.function_counter.get(&fn_id).unwrap().2,
                                        //         PetriNetEdge { label: 1u32 },
                                        //     );
                                        // }
                                        (None, UnwindAction::Cleanup(bb_clean)) => {
                                            self.net.add_edge(
                                                bb_unwind,
                                                *self.bb_node_start_end.get(bb_clean).unwrap(),
                                                PetriNetEdge { label: 1u32 },
                                            );
                                        }
                                        _ => {}
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

                            let bb_unwind_name =
                                fn_name.clone() + &format!("{:?}", bb_idx) + "unwind";
                            let bb_unwind_transition = Transition::new(bb_unwind_name, (0, 0), 1);
                            let bb_unwind =
                                self.net.add_node(PetriNetNode::T(bb_unwind_transition));

                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_end,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.net.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                bb_unwind,
                                PetriNetEdge { label: 1u32 },
                            );
                            self.bb_node_vec
                                .get_mut(&bb_idx)
                                .unwrap()
                                .push(bb_end.clone());
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
                                            PetriNetEdge { label: 1u32 },
                                        );
                                    }
                                    _ => {
                                        self.net.add_edge(
                                            bb_end,
                                            *lock_node,
                                            PetriNetEdge { label: 10u32 },
                                        );
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
                                        PetriNetEdge { label: 1u32 },
                                    );
                                    self.net.add_edge(
                                        bb_unwind,
                                        self.program_panic,
                                        PetriNetEdge { label: 1u32 },
                                    );
                                }
                                // (return_block, UnwindAction::Terminate(_)) => {
                                //     self.net.add_edge(
                                //         bb_end,
                                //         *self.bb_node_start_end.get(return_block).unwrap(),
                                //         PetriNetEdge { label: 1u32 },
                                //     );
                                //     self.net.add_edge(
                                //         bb_unwind,
                                //         self.function_counter.get(&fn_id).unwrap().2,
                                //         PetriNetEdge { label: 1u32 },
                                //     );
                                // }
                                (return_block, UnwindAction::Cleanup(bb_clean)) => {
                                    self.net.add_edge(
                                        bb_end,
                                        *self.bb_node_start_end.get(return_block).unwrap(),
                                        PetriNetEdge { label: 1u32 },
                                    );
                                    self.net.add_edge(
                                        bb_unwind,
                                        self.program_panic,
                                        PetriNetEdge { label: 1u32 },
                                    );
                                }

                                _ => {}
                            }
                        }
                        TerminatorKind::Yield { .. } => {
                            unimplemented!("TerminatorKind::Yield not implemented yet")
                        }
                        TerminatorKind::GeneratorDrop => {
                            unimplemented!("TerminatorKind::GeneratorDrop not implemented yet")
                        }
                        TerminatorKind::FalseEdge { .. } => {
                            unimplemented!("TerminatorKind::FalseEdge not implemented yet")
                        }
                        TerminatorKind::FalseUnwind { .. } => {
                            unimplemented!("TerminatorKind::FalseUnwind not implemented yet")
                        }
                        TerminatorKind::InlineAsm { .. } => {
                            unimplemented!("TerminatorKind::InlineAsm not implemented yet")
                        }
                    }
                    // println!("  terminator: {:?}", term);
                }
            }
        }
    }
}
