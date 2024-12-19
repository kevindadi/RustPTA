use super::{
    callgraph::{CallGraph, InstanceId},
    pn::{ControlType, PetriNetEdge, PetriNetNode, Place, PlaceType, TransitionType},
};
use crate::{
    concurrency::{
        atomic::AtomicOrdering,
        candvar::CondVarId,
        locks::{LockGuardId, LockGuardMap, LockGuardTy},
    },
    graph::pn::Transition,
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    options::Options,
    utils::format_name,
};
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use regex::Regex;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::Body,
    ty::{Instance, TyCtxt},
};
use rustc_middle::{
    mir::{visit::Visitor, BasicBlock, Operand, TerminatorKind},
    ty,
};
use rustc_span::source_map::Spanned;
use std::{cell::RefCell, collections::HashMap};

/// 基于函数的控制流图(CFG)构建Petri网
/// 该结构体负责将Rust MIR中的基本块(Basic Block)转换为Petri网表示
/// 主要用于并发分析，处理锁、条件变量等同步原语
pub struct BodyToPetriNet<'translate, 'analysis, 'tcx> {
    instance_id: InstanceId,              // 函数实例ID
    instance: &'translate Instance<'tcx>, // 函数实例
    body: &'translate Body<'tcx>,         // 函数体MIR
    tcx: TyCtxt<'tcx>,                    // 类型上下文
    options: &'translate Options,         // 配置选项
    callgraph: &'translate CallGraph<'tcx>,
    pub net: &'translate mut Graph<PetriNetNode, PetriNetEdge>, // Petri网图结构
    alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>, // 别名分析
    pub lockguards: LockGuardMap<'tcx>,                         // 锁Guard映射
    function_counter: &'translate HashMap<DefId, (NodeIndex, NodeIndex)>, // 函数节点映射
    locks_counter: &'translate HashMap<LockGuardId, NodeIndex>, // 锁ID映射
    bb_node_start_end: HashMap<BasicBlock, NodeIndex>,          // 基本块起始节点映射
    bb_node_vec: HashMap<BasicBlock, Vec<NodeIndex>>,           // 基本块节点列表
    condvar_id: &'translate HashMap<CondVarId, NodeIndex>,      // 条件变量ID映射
    atomic_load_re: Regex,
    atomic_store_re: Regex,
    atomic_places: &'translate HashMap<AliasId, NodeIndex>,
    atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
}

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        options: &'translate Options,
        // param_env: ParamEnv<'tcx>,
        callgraph: &'translate CallGraph<'tcx>,
        net: &'translate mut Graph<PetriNetNode, PetriNetEdge>,
        // callgraph: &'analysis CallGraph<'tcx>,
        alias: &'translate mut RefCell<AliasAnalysis<'analysis, 'tcx>>,
        lockguards: LockGuardMap<'tcx>,
        function_counter: &'translate HashMap<DefId, (NodeIndex, NodeIndex)>,
        locks_counter: &'translate HashMap<LockGuardId, NodeIndex>,
        // thread_id_handler: &'translate mut HashMap<usize, Vec<JoinHanderId>>,
        // handler_id: &'translate mut HashMap<JoinHanderId, DefId>,
        condvar_id: &'translate HashMap<CondVarId, NodeIndex>,
        atomic_places: &'translate HashMap<AliasId, NodeIndex>,
        atomic_order_maps: &'translate HashMap<AliasId, AtomicOrdering>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            options,
            callgraph,
            net,
            alias,
            lockguards,
            function_counter,
            locks_counter,
            bb_node_start_end: HashMap::default(),
            bb_node_vec: HashMap::new(),
            // thread_id_handler,
            // handler_id,
            condvar_id,
            atomic_load_re: Regex::new(r"atomic[:a-zA-Z0-9]*::load").unwrap(),
            atomic_store_re: Regex::new(r"atomic[:a-zA-Z0-9]*::store").unwrap(),
            atomic_places,
            atomic_order_maps,
        }
    }

    pub fn translate(&mut self) {
        // TODO: 如果函数中不包含同步原语, Skip
        self.visit_body(self.body);
    }

    fn handle_call(
        &mut self,
        bb_idx: BasicBlock,
        fn_name: &str,
        bb_span: &str,
        func: &Operand<'tcx>,
        args: &[Spanned<Operand<'tcx>>],
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
        unwind: &Option<BasicBlock>,
    ) {
        let bb_end = self.create_call_transition(bb_idx, fn_name);

        // 处理锁相关调用
        if self.handle_lock_call(bb_idx, bb_end, destination, target) {
            return;
        }

        // let callee_id = self.get_callee_id(func)?;
        // let callee_name = format_name(callee_id);

        // // 处理线程相关调用
        // if self.handle_thread_call(bb_end, callee_name, args, target) {
        //     return;
        // }

        // // 处理条件变量相关调用
        // if self.handle_condvar_call(bb_end, callee_name, args, target, bb_span) {
        //     return;
        // }

        // // 处理普通函数调用
        // self.handle_normal_call(bb_end, callee_id, target, bb_span);
    }

    fn create_call_transition(&mut self, bb_idx: BasicBlock, fn_name: &str) -> NodeIndex {
        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "call");
        let bb_term_transition =
            Transition::new(bb_term_name, TransitionType::Control(ControlType::Call));
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_start_end.get(&bb_idx).unwrap(),
            bb_end,
            PetriNetEdge { label: 1usize },
        );

        bb_end
    }

    fn handle_lock_call(
        &mut self,
        bb_idx: BasicBlock,
        bb_end: NodeIndex,
        destination: &rustc_middle::mir::Place<'tcx>,
        target: &Option<BasicBlock>,
    ) -> bool {
        // 处理锁相关的逻辑...

        true
    }

    fn handle_thread_call(
        &mut self,
        bb_end: NodeIndex,
        callee_name: String,
        args: &[Spanned<Operand<'tcx>>],
        target: &Option<BasicBlock>,
    ) -> bool {
        // 处理线程相关的逻辑...
        true
    }

    fn handle_condvar_call(
        &mut self,
        bb_end: NodeIndex,
        callee_name: String,
        args: &[Spanned<Operand<'tcx>>],
        target: &Option<BasicBlock>,
        bb_span: &str,
    ) -> bool {
        // 处理条件变量相关的逻辑...
        true
    }

    fn handle_normal_call(
        &mut self,
        bb_end: NodeIndex,
        callee_id: DefId,
        target: &Option<BasicBlock>,
        bb_span: &str,
    ) {
        // 处理普通函数调用的逻辑...
    }
}

impl<'translate, 'analysis, 'tcx> Visitor<'tcx> for BodyToPetriNet<'translate, 'analysis, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let def_id = self.instance.def_id();

        let fn_name = self.tcx.def_path_str(def_id);

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup {
                continue;
            }
            let mut bb_span = String::default();
            if let Some(ref term) = bb.terminator {
                bb_span = format!("{:?}", term.source_info.span);
            } else {
                // debug:检测没有跳转的分支
                bb_span = "".to_string();
            };
            let bb_name = fn_name.clone() + &format!("{:?}", bb_idx);
            let bb_start_place =
                Place::new_with_span(bb_name, 0usize, PlaceType::BasicBlock, bb_span);
            let bb_start = self.net.add_node(PetriNetNode::P(bb_start_place));
            self.bb_node_start_end
                .insert(bb_idx.clone(), bb_start.clone());
            self.bb_node_vec.insert(bb_idx.clone(), vec![bb_start]);
        }
        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            // 不检测cleanup的块，所有的unwind操作忽略
            if bb.is_cleanup {
                continue;
            }

            if bb_idx.index() == 0 {
                let bb_start_name = format!("{}_{}_start", fn_name, bb_idx.index());
                let bb_start_transition = Transition::new(
                    bb_start_name,
                    TransitionType::Control(ControlType::Start(self.instance_id)),
                );
                let bb_start = self.net.add_node(PetriNetNode::T(bb_start_transition));

                self.net.add_edge(
                    self.function_counter.get(&def_id).unwrap().0,
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
                let bb_span = format!("{:?}", term.source_info.span);
                match &term.kind {
                    TerminatorKind::Goto { target } => {
                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "goto");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Basic),
                        );
                        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                        self.net.add_edge(
                            *self.bb_node_start_end.get(&bb_idx).unwrap(),
                            bb_end,
                            PetriNetEdge { label: 1usize },
                        );

                        let target_bb_start = self.bb_node_start_end.get(&target).unwrap();
                        self.net
                            .add_edge(bb_end, *target_bb_start, PetriNetEdge { label: 1usize });
                    }
                    TerminatorKind::SwitchInt { discr: _, targets } => {
                        let mut t_num = 1usize;
                        for t in targets.all_targets() {
                            let bb_term_name =
                                format!("{}_{}_{}", fn_name, bb_idx.index(), "switch")
                                    + "switch"
                                    + t_num.to_string().as_str();
                            t_num += 1;
                            let bb_term_transition = Transition::new(
                                bb_term_name,
                                TransitionType::Control(ControlType::Branch),
                            );
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
                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "resume");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Basic),
                        );
                        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                        self.net.add_edge(
                            *self.bb_node_start_end.get(&bb_idx).unwrap(),
                            bb_end,
                            PetriNetEdge { label: 1usize },
                        );
                        let return_node = self.function_counter.get(&def_id).unwrap().1;
                        self.net
                            .add_edge(bb_end, return_node, PetriNetEdge { label: 1usize });
                    }
                    TerminatorKind::UnwindTerminate(_) => {}
                    TerminatorKind::Return => {
                        let return_node = self.function_counter.get(&def_id).unwrap().1;
                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "return");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Return(self.instance_id)),
                        );
                        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
                        self.net.add_edge(
                            *self.bb_node_start_end.get(&bb_idx).unwrap(),
                            bb_end,
                            PetriNetEdge { label: 1usize },
                        );

                        self.net
                            .add_edge(bb_end, return_node, PetriNetEdge { label: 1usize });
                    }
                    TerminatorKind::Unreachable => {}
                    TerminatorKind::Assert { target, .. } => {
                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "assert");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Basic),
                        );
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

                        let lockguard_id = LockGuardId::new(self.instance_id, destination.local);
                        // let handle_id = JoinHanderId::new(self.instance_id, destination.local);

                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "call");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Call),
                        );
                        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

                        self.net.add_edge(
                            *self.bb_node_start_end.get(&bb_idx).unwrap(),
                            bb_end,
                            PetriNetEdge { label: 1usize },
                        );

                        // 如果当前调用返回的是一个Guard, 则将Guard的节点连接到当前BB的结束节点
                        if let Some(_) = self.lockguards.get_mut(&lockguard_id) {
                            let lock_node = self.locks_counter.get(&lockguard_id).unwrap();
                            match &self.lockguards[&lockguard_id].lockguard_ty {
                                LockGuardTy::StdMutex(_)
                                | LockGuardTy::ParkingLotMutex(_)
                                | LockGuardTy::SpinMutex(_) => {
                                    self.net.add_edge(
                                        *lock_node,
                                        bb_end,
                                        PetriNetEdge { label: 1usize },
                                    );

                                    match self.net.node_weight_mut(bb_end) {
                                        Some(PetriNetNode::T(t)) => {
                                            t.transition_type = TransitionType::Lock;
                                        }
                                        _ => {}
                                    }
                                }

                                LockGuardTy::StdRwLockRead(_)
                                | LockGuardTy::ParkingLotRead(_)
                                | LockGuardTy::SpinRead(_) => {
                                    self.net.add_edge(
                                        *lock_node,
                                        bb_end,
                                        PetriNetEdge { label: 1usize },
                                    );

                                    match self.net.node_weight_mut(bb_end) {
                                        Some(PetriNetNode::T(t)) => {
                                            t.transition_type = TransitionType::RwLockRead;
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {
                                    self.net.add_edge(
                                        *lock_node,
                                        bb_end,
                                        PetriNetEdge { label: 10usize },
                                    );
                                    match self.net.node_weight_mut(bb_end) {
                                        Some(PetriNetNode::T(t)) => {
                                            t.transition_type = TransitionType::RwLockWrite;
                                        }
                                        _ => {}
                                    }
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
                            let callee_ty = func.ty(self.body, self.tcx);

                            let callee_id = match callee_ty.kind() {
                                rustc_middle::ty::TyKind::FnPtr(..) => {
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

                            // 如果当前调用返回的不是Guard, 则将当前BB的结束节点连接到被调用函数的开始节点
                            // 如果当前调用的参数是一个JoinHandle, 则在本函数中查找spawn的返回节点，
                            // 进行匹配，以找到joinhandler对应的def_id
                            // 将当前BB的结束节点连接到被调用函数的开始节点
                            // 判断Caller是nofity或者wait
                            let callee_func_name = format_name(callee_id);

                            if callee_func_name.contains("::spawn") {
                                if let Some(closure_arg) = args.first() {
                                    if let Operand::Move(place) | Operand::Copy(place) =
                                        closure_arg.node
                                    {
                                        let place_ty = place.ty(self.body, self.tcx).ty;
                                        if let ty::Closure(closure_def_id, _) = place_ty.kind() {
                                            self.net.add_edge(
                                                bb_end,
                                                self.function_counter
                                                    .get(&closure_def_id)
                                                    .unwrap()
                                                    .0,
                                                PetriNetEdge { label: 1usize },
                                            );
                                        }
                                        match self.net.node_weight_mut(bb_end) {
                                            Some(PetriNetNode::T(t)) => {
                                                t.transition_type =
                                                    TransitionType::Control(ControlType::Spawn);
                                            }
                                            _ => {}
                                        }
                                        match target {
                                            Some(t) => {
                                                self.net.add_edge(
                                                    bb_end,
                                                    *self.bb_node_start_end.get(t).unwrap(),
                                                    PetriNetEdge { label: 1usize },
                                                );
                                            }
                                            _ => {}
                                        }
                                        continue;
                                    }
                                }
                            }
                            // 链接JoinHandler
                            else if callee_func_name.contains("::join") {
                                // JoinId是caller中传递给Join方法的参数
                                let join_id = AliasId::new(
                                    self.instance_id,
                                    args.get(0).unwrap().node.place().unwrap().local,
                                );
                                match self.callgraph.get_spawn_calls(def_id) {
                                    Some(spawn_call_ids) => {
                                        let mut spawn_def_id = Option::<DefId>::None;
                                        for spawn_call_id in spawn_call_ids.iter() {
                                            // SpawnId是callee中返回的JoinHandler的id
                                            let spawn_local_id =
                                                AliasId::new(self.instance_id, spawn_call_id.1);
                                            spawn_def_id = match self
                                                .alias
                                                .borrow_mut()
                                                .alias(join_id.into(), spawn_local_id.into())
                                            {
                                                ApproximateAliasKind::Probably
                                                | ApproximateAliasKind::Possibly => {
                                                    // log::info!(
                                                    //     "alias between join and spawn: {:?} and {:?}",
                                                    //     join_id,
                                                    //     spawn_local_id
                                                    // );
                                                    Some(spawn_call_id.0)
                                                }
                                                _ => {
                                                    log::info!("no alias between join and spawn");
                                                    continue;
                                                }
                                            };
                                        }
                                        match self.net.node_weight_mut(bb_end) {
                                            Some(PetriNetNode::T(t)) => {
                                                t.transition_type =
                                                    TransitionType::Control(ControlType::Join);
                                            }
                                            _ => {}
                                        }
                                        match spawn_def_id {
                                            Some(s_def_id) => {
                                                self.net.add_edge(
                                                    self.function_counter.get(&s_def_id).unwrap().1,
                                                    bb_end,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                match target {
                                                    Some(t) => {
                                                        self.net.add_edge(
                                                            bb_end,
                                                            *self.bb_node_start_end.get(t).unwrap(),
                                                            PetriNetEdge { label: 1usize },
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            _ => {
                                                log::error!(
                                                    "no spawn call in function {:?}",
                                                    def_id
                                                );
                                                continue;
                                            }
                                        }
                                    }
                                    _ => {
                                        panic!("no spawn call in function {:?}", def_id);
                                    }
                                }
                                continue;
                            }

                            if callee_func_name.contains("::load") {
                                // 处理 atomic load
                                let current_id = AliasId::new(
                                    self.instance_id,
                                    args.get(0).unwrap().node.place().unwrap().local,
                                );
                                for atomic_e in self.atomic_places.iter() {
                                    match self
                                        .alias
                                        .borrow_mut()
                                        .alias(current_id.into(), atomic_e.0.clone().into())
                                    {
                                        ApproximateAliasKind::Possibly
                                        | ApproximateAliasKind::Probably => {
                                            // 创建新的库所和变迁对表示atomic load
                                            let atomic_load_place = Place::new_with_span(
                                                format!(
                                                    "atomic_load_in_{:?}_{:?}",
                                                    current_id.instance_id.index(),
                                                    bb_idx.index()
                                                ),
                                                0,
                                                PlaceType::BasicBlock,
                                                bb_span.clone(),
                                            );
                                            let atomic_load_place_node = self
                                                .net
                                                .add_node(PetriNetNode::P(atomic_load_place));

                                            let _ = self.net.add_edge(
                                                bb_end,
                                                atomic_load_place_node,
                                                PetriNetEdge { label: 1usize },
                                            );

                                            if let Some(order) =
                                                self.atomic_order_maps.get(&current_id)
                                            {
                                                let atomic_load_transition = Transition::new(
                                                    format!(
                                                        "atomic_{:?}_load_{:?}_{:?}",
                                                        self.instance_id.index(),
                                                        order,
                                                        bb_idx.index()
                                                    ),
                                                    TransitionType::AtomicLoad(
                                                        atomic_e.0.clone().into(),
                                                        order.clone(),
                                                        bb_span.clone(),
                                                    ),
                                                );
                                                let atomic_load_transition_node =
                                                    self.net.add_node(PetriNetNode::T(
                                                        atomic_load_transition,
                                                    ));
                                                self.net.add_edge(
                                                    atomic_load_place_node,
                                                    atomic_load_transition_node,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                self.net.add_edge(
                                                    atomic_load_transition_node,
                                                    *atomic_e.1,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                self.net.add_edge(
                                                    *atomic_e.1,
                                                    atomic_load_transition_node,
                                                    PetriNetEdge { label: 1usize },
                                                );

                                                match target {
                                                    Some(t) => {
                                                        self.net.add_edge(
                                                            atomic_load_transition_node,
                                                            *self.bb_node_start_end.get(t).unwrap(),
                                                            PetriNetEdge { label: 1usize },
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }

                                        _ => continue,
                                    }
                                }
                                continue;
                            } else if callee_func_name.contains("::store") {
                                // 处理 atomic store
                                let current_id = AliasId::new(
                                    self.instance_id,
                                    args.get(0).unwrap().node.place().unwrap().local,
                                );
                                for atomic_e in self.atomic_places.iter() {
                                    match self
                                        .alias
                                        .borrow_mut()
                                        .alias(current_id.into(), atomic_e.0.clone().into())
                                    {
                                        ApproximateAliasKind::Possibly
                                        | ApproximateAliasKind::Probably => {
                                            let atomic_store_place = Place::new_with_span(
                                                format!(
                                                    "atomic_store_in_{:?}_{:?}",
                                                    current_id.instance_id.index(),
                                                    bb_idx.index()
                                                ),
                                                0,
                                                PlaceType::BasicBlock,
                                                bb_span.clone(),
                                            );
                                            let atomic_store_place_node = self
                                                .net
                                                .add_node(PetriNetNode::P(atomic_store_place));

                                            self.net.add_edge(
                                                bb_end,
                                                atomic_store_place_node,
                                                PetriNetEdge { label: 1usize },
                                            );

                                            if let Some(order) =
                                                self.atomic_order_maps.get(&current_id)
                                            {
                                                let atomic_store_transition = Transition::new(
                                                    format!(
                                                        "atomic_{:?}_store_{:?}_{:?}",
                                                        self.instance_id.index(),
                                                        order,
                                                        bb_idx.index()
                                                    ),
                                                    TransitionType::AtomicStore(
                                                        atomic_e.0.clone().into(),
                                                        order.clone(),
                                                        bb_span.clone(),
                                                    ),
                                                );
                                                let atomic_store_transition_node =
                                                    self.net.add_node(PetriNetNode::T(
                                                        atomic_store_transition,
                                                    ));
                                                self.net.add_edge(
                                                    atomic_store_place_node,
                                                    atomic_store_transition_node,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                self.net.add_edge(
                                                    atomic_store_transition_node,
                                                    *atomic_e.1,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                self.net.add_edge(
                                                    *atomic_e.1,
                                                    atomic_store_transition_node,
                                                    PetriNetEdge { label: 1usize },
                                                );
                                                match target {
                                                    Some(t) => {
                                                        self.net.add_edge(
                                                            atomic_store_transition_node,
                                                            *self.bb_node_start_end.get(t).unwrap(),
                                                            PetriNetEdge { label: 1usize },
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            } else {
                                                log::info!(
                                                    "No ordering found for {:?}",
                                                    atomic_e.0
                                                );
                                            }
                                        }
                                        _ => continue,
                                    }
                                }
                                continue;
                            }

                            // 如果当前调用的是Condvar::notify, 则将当前BB的结束节点连接到Condvar的节点
                            if callee_func_name.contains("Condvar::notify") {
                                let condvar_local =
                                    args.get(0).unwrap().node.place().unwrap().local;
                                let condvar_id = CondVarId::new(self.instance_id, condvar_local);
                                log::info!("condvar nofity: {:?}", condvar_id);
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
                                match self.net.node_weight_mut(bb_end) {
                                    Some(PetriNetNode::T(t)) => {
                                        t.transition_type = TransitionType::Notify;
                                    }
                                    _ => {}
                                }
                                continue;
                            } else if callee_func_name.contains("Condvar::wait") {
                                let bb_wait_name =
                                    format!("{}_{}_{}", fn_name, bb_idx.index(), "wait");

                                let bb_wait_place = Place::new_with_span(
                                    bb_wait_name,
                                    0,
                                    PlaceType::BasicBlock,
                                    bb_span,
                                );
                                let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                                let bb_ret_name =
                                    format!("{}_{}_{}", fn_name, bb_idx.index(), "ret");
                                let bb_ret_transition =
                                    Transition::new(bb_ret_name, TransitionType::Wait);
                                let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

                                self.net
                                    .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1usize });
                                self.net
                                    .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1usize });

                                let condvar_local =
                                    args.get(0).unwrap().node.place().unwrap().local;
                                let condvar_id = CondVarId::new(self.instance_id, condvar_local);
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
                                    args.get(1).unwrap().node.place().unwrap().local,
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
                                continue;
                            }

                            // 如果被调用的函数不属于当前crate,则忽略,直接链接到下一个Block
                            match callee_func_name.starts_with(&self.options.crate_name) {
                                true => {}
                                false => {
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
                                    log::debug!("ignore function not include in main crate!");
                                    continue;
                                }
                            }

                            let bb_wait_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "wait");
                            let bb_wait_place = Place::new_with_span(
                                bb_wait_name,
                                0,
                                PlaceType::BasicBlock,
                                bb_span,
                            );
                            let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

                            let bb_ret_name =
                                format!("{}_{}_{}", fn_name, bb_idx.index(), "return");
                            let bb_ret_transition = Transition::new(
                                bb_ret_name,
                                TransitionType::Control(ControlType::Call),
                            );
                            let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

                            self.net
                                .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1usize });
                            self.net
                                .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1usize });

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
                                            *self.bb_node_start_end.get(return_block).unwrap(),
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
                                            *self.bb_node_start_end.get(return_block).unwrap(),
                                            PetriNetEdge { label: 1usize },
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
                        unwind: _,
                        replace: _,
                    } => {
                        let bb_term_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "drop");
                        let bb_term_transition = Transition::new(
                            bb_term_name,
                            TransitionType::Control(ControlType::Drop),
                        );
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

                        self.net.add_edge(
                            bb_end,
                            *self.bb_node_start_end.get(target).unwrap(),
                            PetriNetEdge { label: 1usize },
                        );
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
            // }
        }
    }
}
