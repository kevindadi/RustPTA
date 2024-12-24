use super::{
    callgraph::{CallGraph, InstanceId},
    pn::{CallType, ControlType, DropType, PetriNetEdge, PetriNetNode, Place, PlaceType},
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
    mir::{visit::Visitor, BasicBlock, BasicBlockData, Operand, SwitchTargets, TerminatorKind},
    ty,
};
use rustc_middle::{
    mir::{Body, Terminator},
    ty::{Instance, TyCtxt},
};
use rustc_span::Symbol;
use rustc_span::{
    source_map::Spanned,
    sym::{self, sym},
    Span,
};
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

    fn init_basic_block(&mut self, body: &Body<'tcx>, body_name: &str) {
        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup {
                continue;
            }
            let bb_span = bb.terminator.as_ref().map_or("".to_string(), |term| {
                format!("{:?}", term.source_info.span)
            });

            let bb_name = format!("{}_{}", body_name, bb_idx.index());
            let bb_start_place =
                Place::new_with_span(bb_name, 0usize, PlaceType::BasicBlock, bb_span);
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
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } => self.handle_call(
                bb_idx,
                func,
                args,
                destination,
                target,
                name,
                &format!("{:?}", term.source_info.span),
            ),
            TerminatorKind::Drop { place, target, .. } => {
                self.handle_drop(bb_idx, place, target, name, bb)
            }
            _ => {}
        }
    }

    fn handle_goto(&mut self, bb_idx: BasicBlock, target: &BasicBlock, name: &str) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "goto");
        let bb_term_transition = Transition::new(bb_term_name, ControlType::Goto);
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

    fn handle_switch(&mut self, bb_idx: BasicBlock, targets: &SwitchTargets, name: &str) {
        let mut t_num = 1usize;
        for t in targets.all_targets() {
            let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "switch")
                + "switch"
                + t_num.to_string().as_str();
            t_num += 1;
            let bb_term_transition = Transition::new(bb_term_name, ControlType::Switch);
            let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

            self.net.add_edge(
                *self.bb_node_start_end.get(&bb_idx).unwrap(),
                bb_end,
                PetriNetEdge { label: 1usize },
            );
            let target_bb_start = self.bb_node_start_end.get(t).unwrap();
            self.net
                .add_edge(bb_end, *target_bb_start, PetriNetEdge { label: 1usize });
        }
    }

    fn handle_return(&mut self, bb_idx: BasicBlock, name: &str) {
        let return_node = self
            .function_counter
            .get(&self.instance.def_id())
            .unwrap()
            .1;
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
        let bb_term_transition =
            Transition::new(bb_term_name, ControlType::Return(self.instance_id));
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));
        self.net.add_edge(
            *self.bb_node_start_end.get(&bb_idx).unwrap(),
            bb_end,
            PetriNetEdge { label: 1usize },
        );

        self.net
            .add_edge(bb_end, return_node, PetriNetEdge { label: 1usize });
    }

    fn create_call_transition(&mut self, bb_idx: BasicBlock, bb_term_name: &str) -> NodeIndex {
        let bb_term_transition = Transition::new(
            bb_term_name.to_string(),
            ControlType::Call(CallType::Function),
        );
        let bb_end = self.net.add_node(PetriNetNode::T(bb_term_transition));

        self.net.add_edge(
            *self.bb_node_start_end.get(&bb_idx).unwrap(),
            bb_end,
            PetriNetEdge { label: 1 },
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
        // 1. 更新变迁类型
        if let Some(PetriNetNode::T(transition)) = self.net.node_weight_mut(bb_end) {
            transition.transition_type = ControlType::Call(call_type.clone());
        }

        // 2. 根据不同的锁类型添加边
        match call_type {
            CallType::Lock(_) | CallType::RwLockRead(_) => {
                // 互斥锁和读锁消耗一个token
                self.net
                    .add_edge(*lock_node, bb_end, PetriNetEdge { label: 1 });
            }
            CallType::RwLockWrite(_) => {
                // 写锁消耗全部token
                self.net
                    .add_edge(*lock_node, bb_end, PetriNetEdge { label: 10 });
            }
            _ => {}
        }
    }

    fn connect_to_target(&mut self, bb_end: NodeIndex, target: &Option<BasicBlock>) {
        if let Some(target_bb) = target {
            self.net.add_edge(
                bb_end,
                *self.bb_node_start_end.get(target_bb).unwrap(),
                PetriNetEdge { label: 1usize },
            );
        }
    }

    fn handle_thread_call(
        &mut self,
        callee_func_name: &str,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) -> bool {
        if callee_func_name.contains("thread::spawn") {
            self.handle_spawn(args, target, bb_end);
            true
        } else if callee_func_name.contains("::join") {
            self.handle_join(args, target, bb_end);
            true
        } else {
            false
        }
    }

    fn handle_spawn(
        &mut self,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) {
        if let Some(closure_arg) = args.first() {
            if let Operand::Move(place) | Operand::Copy(place) = closure_arg.node {
                let place_ty = place.ty(self.body, self.tcx).ty;
                if let ty::Closure(closure_def_id, _) | ty::FnDef(closure_def_id, _) =
                    place_ty.kind()
                {
                    self.net.add_edge(
                        bb_end,
                        self.function_counter.get(&closure_def_id).unwrap().0,
                        PetriNetEdge { label: 1usize },
                    );
                }
                match self.net.node_weight_mut(bb_end) {
                    Some(PetriNetNode::T(t)) => {
                        t.transition_type = ControlType::Call(CallType::Spawn);
                    }
                    _ => {}
                }
                self.connect_to_target(bb_end, target);
            }
        }
    }

    fn handle_join(
        &mut self,
        args: &Box<[Spanned<Operand<'tcx>>]>,
        target: &Option<BasicBlock>,
        bb_end: NodeIndex,
    ) {
        // 1. 获取join handle的ID
        let join_id = AliasId::new(
            self.instance_id,
            args.first().unwrap().node.place().unwrap().local,
        );

        // 2. 获取spawn调用并查找匹配
        let spawn_calls = self
            .callgraph
            .get_spawn_calls(self.instance.def_id())
            .unwrap();
        let spawn_def_id = spawn_calls
            .iter()
            .find_map(|(def_id, local)| {
                let spawn_local_id = AliasId::new(self.instance_id, *local);
                match self
                    .alias
                    .borrow_mut()
                    .alias_join(join_id.into(), spawn_local_id.into())
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

        // 3. 更新变迁类型并建立连接
        if let Some(PetriNetNode::T(transition)) = self.net.node_weight_mut(bb_end) {
            transition.transition_type = ControlType::Call(CallType::Join);
        }

        // 4. 连接spawn结束到join
        self.net.add_edge(
            self.function_counter.get(&spawn_def_id.unwrap()).unwrap().1,
            bb_end,
            PetriNetEdge { label: 1 },
        );

        // 5. 连接到目标基本块
        self.connect_to_target(bb_end, target);
    }

    fn handle_normal_call(
        &mut self,
        callee_func_name: &str,
        bb_end: NodeIndex,
        target: &Option<BasicBlock>,
        name: &str,
        bb_idx: BasicBlock,
        span: &str,
        callee_id: &DefId,
    ) {
        match callee_func_name.starts_with(&self.options.crate_name) {
            true => {}
            false => {
                match target {
                    Some(return_block) => {
                        self.net.add_edge(
                            bb_end,
                            *self.bb_node_start_end.get(return_block).unwrap(),
                            PetriNetEdge { label: 1usize },
                        );
                    }
                    _ => {}
                }
                log::debug!("ignore function not include in main crate!");
                return;
            }
        }

        let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
        let bb_wait_place =
            Place::new_with_span(bb_wait_name, 0, PlaceType::BasicBlock, span.to_string());
        let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

        let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "return");
        let bb_ret_transition = Transition::new(bb_ret_name, ControlType::Call(CallType::Function));
        let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

        self.net
            .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1usize });
        self.net
            .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1usize });

        if let Some((callee_start, callee_end)) = self.function_counter.get(callee_id) {
            self.net
                .add_edge(bb_end, *callee_start, PetriNetEdge { label: 1usize });
            match target {
                Some(return_block) => {
                    self.net
                        .add_edge(*callee_end, bb_ret, PetriNetEdge { label: 1usize });
                    self.net.add_edge(
                        bb_ret,
                        *self.bb_node_start_end.get(return_block).unwrap(),
                        PetriNetEdge { label: 1usize },
                    );
                }
                _ => {}
            }
        } else {
            self.connect_to_target(bb_ret, target);
        }
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
            self.handle_atomic_load(args, bb_end, target, bb_idx, span)
        } else if callee_func_name.contains("::store") {
            self.handle_atomic_store(args, bb_end, target, bb_idx, span)
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
                    .alias(current_id.into(), atomic_e.0.clone().into()),
                ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably
            ) {
                continue;
            }

            log::info!("atomic load: {:?}", atomic_e.0);

            // 创建load操作的库所
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

            // 创建load操作的变迁
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
                    )),
                );
                let atomic_load_transition_node =
                    self.net.add_node(PetriNetNode::T(atomic_load_transition));

                // 添加边
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
        true
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
                    .alias(current_id.into(), atomic_e.0.clone().into()),
                ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably
            ) {
                continue;
            }

            log::info!("atomic store: {:?}", atomic_e.0);

            // 创建store操作的库所
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

            // 创建store操作的变迁
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
                    )),
                );
                let atomic_store_transition_node =
                    self.net.add_node(PetriNetNode::T(atomic_store_transition));

                // 添加边
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
        true
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

            // 创建compare_exchange操作的库所
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

            // 创建success和failure的变迁
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
                    )),
                );
                let atomic_cmpxchg_transition_node = self
                    .net
                    .add_node(PetriNetNode::T(atomic_cmpxchg_transition));

                // 添加边
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
        // 如果当前调用的是Condvar::notify, 则将当前BB的结束节点连接到Condvar的节点
        if callee_func_name.contains("Condvar::notify") {
            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            log::debug!("condvar notify: {:?}", condvar_id);

            // 查找匹配的条件变量并建立连接
            for (id, node) in self.condvar_id.iter() {
                match self
                    .alias
                    .borrow_mut()
                    .alias_condvar(condvar_id.into(), (*id).into())
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
        } else if callee_func_name.contains("Condvar::wait") {
            // 处理wait调用
            // 1. 创建等待节点和变迁
            let bb_wait_name = format!("{}_{}_{}", name, bb_idx.index(), "wait");
            let bb_wait_place =
                Place::new_with_span(bb_wait_name, 0, PlaceType::BasicBlock, span.to_string());
            let bb_wait = self.net.add_node(PetriNetNode::P(bb_wait_place));

            let bb_ret_name = format!("{}_{}_{}", name, bb_idx.index(), "ret");
            let bb_ret_transition = Transition::new(bb_ret_name, ControlType::Call(CallType::Wait));
            let bb_ret = self.net.add_node(PetriNetNode::T(bb_ret_transition));

            // 2. 建立基本连接
            self.net
                .add_edge(bb_end, bb_wait, PetriNetEdge { label: 1 });
            self.net
                .add_edge(bb_wait, bb_ret, PetriNetEdge { label: 1 });

            // 3. 处理条件变量连接
            let condvar_id = CondVarId::new(
                self.instance_id,
                args.first().unwrap().node.place().unwrap().local,
            );
            log::info!("condvar wait: {:?}", condvar_id);

            for (id, node) in self.condvar_id.iter() {
                match self
                    .alias
                    .borrow_mut()
                    .alias_condvar(condvar_id.into(), (*id).into())
                {
                    ApproximateAliasKind::Possibly | ApproximateAliasKind::Probably => {
                        self.net.add_edge(*node, bb_ret, PetriNetEdge { label: 1 });
                    }
                    _ => continue,
                }
            }

            // 4. 处理关联的锁
            let guard_id = LockGuardId::new(
                self.instance_id,
                args.get(1).unwrap().node.place().unwrap().local,
            );
            let lock_node = self.locks_counter.get(&guard_id).unwrap();
            self.net
                .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1 });
            self.net
                .add_edge(*lock_node, bb_ret, PetriNetEdge { label: 1 });

            // 5. 连接到目标基本块
            self.connect_to_target(bb_ret, target);

            true
        } else {
            false
        }
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
    ) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "call");
        let bb_end = self.create_call_transition(bb_idx, &bb_term_name);
        let callee_ty = func.ty(self.body, self.tcx);
        let callee_def_id = match callee_ty.kind() {
            rustc_middle::ty::TyKind::FnPtr(..) => {
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
        // 1. 处理锁相关调用
        if let Some(_) = self.handle_lock_call(destination, target, bb_end) {
            return;
        }

        // 2. 处理线程相关调用
        if self.handle_thread_call(&callee_func_name, args, target, bb_end) {
            return;
        }

        // 3. 处理条件变量调用
        if self.handle_condvar_call(&callee_func_name, args, bb_end, target, name, &bb_idx, span) {
            return;
        }

        // 4. 处理原子操作调用
        if self.handle_atomic_call(&callee_func_name, args, bb_end, target, &bb_idx, span) {
            return;
        }

        if callee_func_name.contains("::drop") {
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
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1usize });

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
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1usize });

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
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 10usize });
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
            return;
        }
        // 5. 处理普通函数调用
        self.handle_normal_call(
            &callee_func_name,
            bb_end,
            target,
            name,
            bb_idx,
            span,
            &callee_def_id,
        );
    }

    fn handle_drop(
        &mut self,
        bb_idx: BasicBlock,
        place: &rustc_middle::mir::Place<'tcx>,
        target: &BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        let bb_term_name = format!("{}_{}_{}", name, bb_idx.index(), "drop");
        let bb_term_transition = Transition::new(bb_term_name, ControlType::Drop(DropType::Basic));
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
                        self.net
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 1usize });
                    }
                    _ => {
                        self.net
                            .add_edge(bb_end, *lock_node, PetriNetEdge { label: 10usize });
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
            PetriNetEdge { label: 1usize },
        );
    }
}

impl<'translate, 'analysis, 'tcx> Visitor<'tcx> for BodyToPetriNet<'translate, 'analysis, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let def_id = self.instance.def_id();

        let fn_name = self.tcx.def_path_str(def_id);

        // 初始化基本块, 创建基本块的开始库所
        self.init_basic_block(body, &fn_name);

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            // 不检测cleanup的块，所有的unwind操作忽略
            if bb.is_cleanup {
                continue;
            }

            if bb_idx.index() == 0 {
                self.handle_start_block(&fn_name, bb_idx, def_id);
            }

            // 处理基本块的终止符
            if let Some(term) = &bb.terminator {
                self.handle_terminator(term, bb_idx, &fn_name, bb);
            }
        }
    }
}
