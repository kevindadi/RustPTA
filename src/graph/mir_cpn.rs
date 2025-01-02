use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    options::Options,
    utils::format_name,
};
use petgraph::{graph::NodeIndex, Graph};
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{visit::Visitor, BasicBlock, Operand, Place, Rvalue, StatementKind, TerminatorKind},
    ty,
};
use rustc_middle::{
    mir::{Body, Statement},
    ty::{Instance, TyCtxt},
};

use super::{
    callgraph::{CallGraph, InstanceId},
    cpn::{ColorPetriEdge, ColorPetriNode, DataOp, DataOpType, DataTokenType},
};

pub struct BodyToColorPetriNet<'cpn, 'translate, 'tcx> {
    instance_id: InstanceId,
    instance: &'cpn Instance<'tcx>,
    body: &'cpn Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    options: &'cpn Options,
    callgraph: &'cpn CallGraph<'tcx>,
    net: &'cpn mut Graph<ColorPetriNode, ColorPetriEdge>,
    alias: &'cpn mut RefCell<AliasAnalysis<'translate, 'tcx>>,
    function_counter: &'cpn HashMap<DefId, (NodeIndex, NodeIndex)>,
    bb_node_start_end: HashMap<BasicBlock, NodeIndex>, // 基本块起始节点映射
    bb_node_vec: HashMap<BasicBlock, Vec<NodeIndex>>,  // 基本块节点映射
    unsafe_data: &'cpn FxHashMap<AliasId, String>,
    unsafe_places: &'cpn HashMap<AliasId, NodeIndex>,
}

impl<'cpn, 'translate, 'tcx> BodyToColorPetriNet<'cpn, 'translate, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'cpn Instance<'tcx>,
        body: &'cpn Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        options: &'cpn Options,
        callgraph: &'cpn CallGraph<'tcx>,
        net: &'cpn mut Graph<ColorPetriNode, ColorPetriEdge>,
        alias: &'cpn mut RefCell<AliasAnalysis<'translate, 'tcx>>,
        function_counter: &'cpn HashMap<DefId, (NodeIndex, NodeIndex)>,
        unsafe_data: &'cpn FxHashMap<AliasId, String>,
        unsafe_places: &'cpn HashMap<AliasId, NodeIndex>,
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
            function_counter,
            bb_node_start_end: HashMap::new(),
            bb_node_vec: HashMap::new(),
            unsafe_data,
            unsafe_places,
        }
    }

    pub fn translate(&mut self) {
        let def_id = self.instance.def_id();
        let fn_name = self.tcx.def_path_str(def_id);

        for (bb_idx, bb) in self.body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup {
                continue;
            }
            let bb_name = format!("{}_{}", fn_name, bb_idx.index());
            let bb_start = self.net.add_node(ColorPetriNode::ControlPlace {
                basic_block: bb_name,
                token_num: Arc::new(RwLock::new(0)),
            });
            self.bb_node_start_end
                .insert(bb_idx.clone(), bb_start.clone());
            self.bb_node_vec.insert(bb_idx.clone(), vec![bb_start]);
        }

        self.visit_body(self.body);
    }

    // 添加数据库所
    pub fn add_data_place(&mut self, token_type: &Vec<DataTokenType>) -> NodeIndex {
        self.net.add_node(ColorPetriNode::DataPlace {
            token_type: token_type.clone(),
            token_num: token_type.len(),
        })
    }

    // 添加控制库所
    pub fn add_control_place(&mut self, basic_block: String, token_num: usize) -> NodeIndex {
        self.net.add_node(ColorPetriNode::ControlPlace {
            basic_block,
            token_num: Arc::new(RwLock::new(token_num)),
        })
    }

    // 添加变迁
    fn add_cfg_transition(&mut self, basic_block: String) -> NodeIndex {
        self.net.add_node(ColorPetriNode::Cfg { name: basic_block })
    }

    fn add_unsafe_transition(
        &mut self,
        data_ops: AliasId,
        info: String,
        span: String,
        rw_type: DataOpType,
        basic_block: usize,
    ) -> NodeIndex {
        self.net.add_node(ColorPetriNode::UnsafeTransition {
            data_ops,
            info,
            span,
            rw_type,
            basic_block,
        })
    }

    // fn add_unsafe_block_transition(
    //     &mut self,
    //     data_ops: Vec<DataOp>,
    //     span: String,
    //     rw_type: DataOpType,
    // ) -> NodeIndex {
    //     self.net.add_node(ColorPetriNode::UnsafeTransition {
    //         data_ops,
    //         span,
    //         rw_type,
    //     })
    // }

    // 添加边
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: u32) {
        self.net.add_edge(from, to, ColorPetriEdge { weight });
    }

    fn get_or_insert_bb_entry_node(&mut self, bb_idx: BasicBlock, fn_name: &str) -> NodeIndex {
        if let Some(&node) = self.bb_node_start_end.get(&bb_idx) {
            return node;
        }

        let bb_name = format!("{}-{:?}", fn_name, bb_idx);
        let bb_start = self.add_control_place(bb_name, 0);
        self.bb_node_start_end.insert(bb_idx, bb_start);
        self.bb_node_vec
            .entry(bb_idx)
            .or_insert(vec![])
            .push(bb_start);
        bb_start
    }

    // 处理右值中的读操作
    fn process_rvalue_reads(
        &mut self,
        rvalue: &Rvalue<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let mut data_ops = Vec::new();
        // 访问右值中的所有Place
        // 根据不同的Rvalue类型获取所有相关的Place
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
            // 其他类型的Rvalue根据需要添加
            _ => vec![],
        };

        // 处理所有找到的Place
        for place in places {
            let place_id = AliasId::new(self.instance_id, place.local);
            let place_ty = format!("{:?}", place.ty(self.body, self.tcx));
            // 检查是否与任何unsafe数据存在别名关系
            let alias_result = self.has_unsafe_alias(place_id);
            if alias_result.0 {
                // 创建读操作
                let data_op = DataOp {
                    op_type: DataOpType::Read,
                    data: DataTokenType {
                        ty: place_ty.clone(),
                        local: place.local,
                        def_id: self.instance.def_id(),
                    },
                    thread_name: fn_name.to_string(),
                };
                data_ops.push(data_op.clone());

                let transition_name = format!("{}_read_{}_in:{}", fn_name, place_ty, span_str);
                let transition = self.add_unsafe_transition(
                    alias_result.2.unwrap(),
                    transition_name.clone(),
                    span_str.to_string(),
                    DataOpType::Read,
                    bb_idx.index(),
                );

                // 链接bb的前一个库所
                let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
                self.add_edge(*bb_nodes, transition, 1);

                // 找到Unsafe的库所，TODO:这里直接返回，需要优化Move和Drop的位置
                let unsafe_place = alias_result.1;
                self.add_edge(unsafe_place, transition, 1);
                self.add_edge(transition, unsafe_place, 1);

                // 建立库所，链接terminator
                let place_name = format!("{}_rready", &transition_name.as_str());
                let place = self.add_control_place(place_name, 0);
                self.add_edge(transition, place, 1);

                self.bb_node_vec.get_mut(&bb_idx).unwrap().push(transition);
                self.bb_node_vec.get_mut(&bb_idx).unwrap().push(place);
            }
        }
    }

    // 处理左值的写操作
    fn process_place_writes(
        &mut self,
        place: &Place<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let place_id = AliasId::new(self.instance_id, place.local);
        let place_ty = format!("{:?}", place.ty(self.body, self.tcx));
        // 检查是否与任何unsafe数据存在别名关系
        let alias_result = self.has_unsafe_alias(place_id);
        if alias_result.0 {
            let data_ops = vec![DataOp {
                op_type: DataOpType::Write,
                data: DataTokenType {
                    ty: place_ty.clone(),
                    local: place.local,
                    def_id: self.instance.def_id(),
                },
                thread_name: fn_name.to_string(),
            }];

            let transition_name = format!("{}_write_{}_in:{}", fn_name, place_ty, span_str);
            let transition = self.add_unsafe_transition(
                alias_result.2.unwrap(),
                transition_name.clone(),
                span_str.to_string(),
                DataOpType::Write,
                bb_idx.index(),
            );

            // 链接bb的前一个库所
            let bb_nodes = self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap();
            self.add_edge(*bb_nodes, transition, 1);

            // 找到Unsafe的库所，TODO:这里直接返回，需要优化Move和Drop的位置
            let unsafe_place = alias_result.1;
            self.add_edge(unsafe_place, transition, 1);
            self.add_edge(transition, unsafe_place, 1);

            // 建立库所，链接terminator
            let place_name = format!("{}_wready", &transition_name.as_str());
            let place = self.add_control_place(place_name, 0);
            self.add_edge(transition, place, 1);

            self.bb_node_vec.get_mut(&bb_idx).unwrap().push(transition);
            self.bb_node_vec.get_mut(&bb_idx).unwrap().push(place);
        }
    }

    // 检查是否与unsafe数据存在别名关系
    fn has_unsafe_alias(&self, place_id: AliasId) -> (bool, NodeIndex, Option<AliasId>) {
        for (unsafe_place, _) in self.unsafe_data.iter() {
            match self
                .alias
                .borrow_mut()
                .alias(place_id.into(), *unsafe_place)
            {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    let unsafe_place_node = self.unsafe_places.get(unsafe_place).unwrap();
                    return (true, unsafe_place_node.clone(), Some(unsafe_place.clone()));
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

            // 先处理右值（读操作）
            self.process_rvalue_reads(rvalue, &fn_name, bb_idx, &span_str);

            // 再处理左值（写操作）
            self.process_place_writes(dest, &fn_name, bb_idx, &span_str);
        }
    }
}

impl<'cpn, 'translate, 'tcx> Visitor<'tcx> for BodyToColorPetriNet<'cpn, 'translate, 'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        let def_id = self.instance.def_id();
        let fn_name = self.tcx.def_path_str(def_id);

        for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup {
                continue;
            }

            // deal statement
            for stmt in bb.statements.iter() {
                self.visit_statement_body(stmt, bb_idx);
            }

            if bb_idx.index() == 0 {
                let bb_start_name = format!("{}_{}_start", fn_name, bb_idx.index());
                let bb_start = self.add_cfg_transition(bb_start_name);

                self.add_edge(self.function_counter.get(&def_id).unwrap().0, bb_start, 1);
                let bb_entry = self.get_or_insert_bb_entry_node(bb_idx, &fn_name);
                self.add_edge(bb_start, bb_entry, 1);
            }

            // 处理控制流
            if let Some(ref term) = bb.terminator {
                match &term.kind {
                    TerminatorKind::Goto { target } => {
                        let goto_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "goto");
                        let goto_transition = self.add_cfg_transition(goto_name);

                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            goto_transition,
                            1,
                        );

                        let target_place = self.get_or_insert_bb_entry_node(*target, &fn_name);
                        self.add_edge(goto_transition, target_place, 1);
                    }
                    TerminatorKind::SwitchInt { discr: _, targets } => {
                        let mut t_num = 1usize;
                        for t in targets.all_targets() {
                            let switch_name =
                                format!("{}_{}_switch_{}", fn_name, bb_idx.index(), t_num);
                            t_num += 1;
                            let switch_transition = self.add_cfg_transition(switch_name);

                            self.add_edge(
                                *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                                switch_transition,
                                1,
                            );
                            let target_place = self.get_or_insert_bb_entry_node(*t, &fn_name);
                            self.add_edge(switch_transition, target_place, 1);
                        }
                    }
                    TerminatorKind::UnwindResume => {
                        let unwind_resume = format!("{}_{}_{}", fn_name, bb_idx.index(), "resume");
                        let uw_resume_transition = self.add_cfg_transition(unwind_resume);
                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            uw_resume_transition,
                            1,
                        );
                        let return_node = self.function_counter.get(&def_id).unwrap().1;
                        self.add_edge(uw_resume_transition, return_node, 1);
                    }
                    TerminatorKind::UnwindTerminate(_) => {}
                    TerminatorKind::Return => {
                        let return_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "return");
                        let return_transition = self.add_cfg_transition(return_name);
                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            return_transition,
                            1,
                        );

                        let return_node = self.function_counter.get(&def_id).unwrap().1;
                        self.add_edge(return_transition, return_node, 1);
                    }
                    TerminatorKind::Unreachable => {}
                    TerminatorKind::Assert { target, .. } => {
                        let assert_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "assert");
                        let assert_transition = self.add_cfg_transition(assert_name);

                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            assert_transition,
                            1,
                        );

                        let target_place = self.get_or_insert_bb_entry_node(*target, &fn_name);
                        self.add_edge(assert_transition, target_place, 1);
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
                        let callee_id = match call_ty {
                            rustc_middle::ty::TyKind::FnPtr(..) => {
                                return;
                            }
                            rustc_middle::ty::TyKind::FnDef(def_id, _)
                            | rustc_middle::ty::TyKind::Closure(def_id, _) => *def_id,
                            _ => {
                                panic!(
                                    "TyKind::FnDef, a function definition, but got: {call_ty:?}"
                                );
                            }
                        };

                        let call_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "call");
                        let call_transition = self.add_cfg_transition(call_name);

                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            call_transition,
                            1,
                        );

                        let callee_func_name = format_name(callee_id);

                        if callee_func_name.contains("::spawn") {
                            if let Some(closure_arg) = args.first() {
                                let closure_ty = match closure_arg.node {
                                    Operand::Move(place) | Operand::Copy(place) => {
                                        place.ty(self.body, self.tcx).ty
                                    }
                                    Operand::Constant(ref const_op) => const_op.ty(),
                                };

                                if let ty::Closure(closure_def_id, _)
                                | ty::FnDef(closure_def_id, _) = closure_ty.kind()
                                {
                                    self.add_edge(
                                        call_transition,
                                        self.function_counter.get(&closure_def_id).unwrap().0,
                                        1,
                                    );
                                }
                                match target {
                                    Some(t) => {
                                        let target_place =
                                            self.get_or_insert_bb_entry_node(*t, &fn_name);
                                        self.add_edge(call_transition, target_place, 1);
                                    }
                                    _ => {}
                                }
                                continue;
                            }
                        } else if callee_func_name.contains("::join") {
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
                                            .alias_join(join_id.into(), spawn_local_id.into())
                                        {
                                            ApproximateAliasKind::Probably
                                            | ApproximateAliasKind::Possibly => {
                                                Some(spawn_call_id.0)
                                            }
                                            _ => {
                                                continue;
                                            }
                                        };
                                    }
                                    match spawn_def_id {
                                        Some(s_def_id) => {
                                            self.add_edge(
                                                self.function_counter.get(&s_def_id).unwrap().1,
                                                call_transition,
                                                1,
                                            );
                                        }
                                        _ => {
                                            log::error!("no spawn call in function {:?}", def_id);
                                        }
                                    }
                                }
                                _ => {
                                    panic!("no spawn call in function {:?}", def_id);
                                }
                            }
                            match target {
                                Some(t) => {
                                    let target_place =
                                        self.get_or_insert_bb_entry_node(*t, &fn_name);
                                    self.add_edge(call_transition, target_place, 1);
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // 如果被调用的函数不属于当前crate,则忽略,直接链接到下一个Block
                        match callee_func_name.contains(&self.options.crate_name) {
                            true => {}
                            false => {
                                match (target, unwind) {
                                    (Some(return_block), _) => {
                                        let target_place = self
                                            .get_or_insert_bb_entry_node(*return_block, &fn_name);
                                        self.add_edge(call_transition, target_place, 1);
                                    }
                                    _ => {}
                                }
                                log::debug!("ignore function not include in main crate!");
                                continue;
                            }
                        }

                        // 函数调用建模
                        let wait_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "wait");
                        let wait_place = self.add_control_place(wait_name, 0);

                        let ret_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "return");
                        let ret_transition = self.add_cfg_transition(ret_name);

                        self.add_edge(call_transition, wait_place, 1);
                        self.add_edge(wait_place, ret_transition, 1);

                        if let Some((callee_start, callee_end)) =
                            self.function_counter.get(&callee_id)
                        {
                            self.add_edge(call_transition, *callee_start, 1);
                            match (target, unwind) {
                                (Some(target_block), _) => {
                                    let target_place =
                                        self.get_or_insert_bb_entry_node(*target_block, &fn_name);
                                    self.add_edge(*callee_end, ret_transition, 1);
                                    self.add_edge(ret_transition, target_place, 1);
                                }
                                _ => {}
                            }
                        } else {
                            match (target, unwind) {
                                (Some(target_block), _) => {
                                    let target_place =
                                        self.get_or_insert_bb_entry_node(*target_block, &fn_name);
                                    self.add_edge(ret_transition, target_place, 1);
                                }
                                _ => {}
                            }
                        }
                    }
                    TerminatorKind::Drop {
                        place,
                        target,
                        unwind: _,
                        replace: _,
                    } => {
                        let drop_name = format!("{}_{}_{}", fn_name, bb_idx.index(), "drop");
                        let drop_transition = self.add_cfg_transition(drop_name);

                        self.add_edge(
                            *self.bb_node_vec.get(&bb_idx).unwrap().last().unwrap(),
                            drop_transition,
                            1,
                        );

                        let target_place = self.get_or_insert_bb_entry_node(*target, &fn_name);
                        self.add_edge(drop_transition, target_place, 1);
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
