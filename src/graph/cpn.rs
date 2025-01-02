use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::IntoNodeReferences,
    Direction,
};
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Local};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
};

use crate::{
    memory::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    memory::unsafe_memory::UnsafeData,
    options::Options,
    utils::format_name,
};

use super::{
    callgraph::{CallGraph, CallGraphNode},
    mir_cpn::BodyToColorPetriNet,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CpnStructureError {
    #[error("UnsafeTransition at {span} is missing required predecessors:\n- Has control place: {has_control}\n- Has data place: {has_data}")]
    UnsafeTransitionMissingPredecessors {
        span: String,
        has_control: bool,
        has_data: bool,
    },

    #[error("UnsafeTransition at {span} has invalid predecessor type: {found_type}")]
    UnsafeTransitionInvalidPredecessor { span: String, found_type: String },

    #[error("UnsafeTransition at {span} must have at least one control place successor")]
    UnsafeTransitionMissingControlSuccessor { span: String },

    #[error("UnsafeTransition at {span} has invalid successor type: {found_type}")]
    UnsafeTransitionInvalidSuccessor { span: String, found_type: String },

    #[error(
        "Cfg transition '{name}' must only have ControlPlace predecessors, found: {found_type}"
    )]
    CfgInvalidPredecessor { name: String, found_type: String },

    #[error("Cfg transition '{name}' must only have ControlPlace successors, found: {found_type}")]
    CfgInvalidSuccessor { name: String, found_type: String },

    #[error("{place_type} at {basic_block} has invalid predecessor type: {found_type}")]
    PlaceInvalidPredecessor {
        place_type: String,
        basic_block: String,
        found_type: String,
    },

    #[error("{place_type} at {basic_block} has invalid successor type: {found_type}")]
    PlaceInvalidSuccessor {
        place_type: String,
        basic_block: String,
        found_type: String,
    },
}

/// 着色Petri网的基本结构
pub struct ColorPetriNet<'analysis, 'tcx> {
    options: Options,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    pub net: DiGraph<ColorPetriNode, ColorPetriEdge>,
    callgraph: &'analysis CallGraph<'tcx>,
    alias: RefCell<AliasAnalysis<'analysis, 'tcx>>,
    function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    // 当前标识
    marking: HashMap<NodeIndex, usize>,
    pub entry_node: NodeIndex,
    pub unsafe_data: UnsafeData,
    pub unsafe_places: HashMap<AliasId, NodeIndex>,
}

/// Petri网节点类型
#[derive(Debug, Clone)]
pub enum ColorPetriNode {
    // 数据库所: 存储所有unsafe数据
    DataPlace {
        // 存储所有unsafe数据的token
        token_type: Vec<DataTokenType>,
        // 存储所有unsafe数据token的数量
        token_num: usize,
    },
    TempDataPlace {
        basic_block: String,
        local: usize,
        token_num: Arc<RwLock<usize>>,
    },
    // 控制库所: 表示程序执行到某个位置
    ControlPlace {
        basic_block: String,
        token_num: Arc<RwLock<usize>>,
    },
    // 变迁: 表示基本块中的操作
    UnsafeTransition {
        // TODO:变迁中包含的所有数据操作
        // data_ops: Vec<DataOp>,
        data_ops: AliasId,
        info: String,
        // 变迁所在的基本块
        span: String,
        rw_type: DataOpType,
        basic_block: usize,
    },
    Cfg {
        name: String,
    },
}

/// Unsafe数据的Token类型
#[derive(Debug, Clone, PartialEq)]
pub struct DataTokenType {
    // 变量类型
    pub(crate) ty: String,
    // 标识符
    pub(crate) local: Local,
    pub(crate) def_id: DefId,
}

/// Token
#[derive(Debug, Clone)]
pub enum Token {
    // 数据token
    Data {
        token_type: DataTokenType,
        value: String,
    },
    // 控制token
    Control {
        basic_block: BasicBlock,
    },
}

/// 数据操作
#[derive(Debug, Clone)]
pub struct DataOp {
    // 操作类型(读/写)
    pub op_type: DataOpType,
    // 操作的数据
    pub data: DataTokenType,
    // 操作的线程
    pub thread_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum DataOpType {
    Read,
    Write,
}

/// Petri网边
#[derive(Debug, Clone)]
pub struct ColorPetriEdge {
    pub weight: u32,
}

impl<'analysis, 'tcx> ColorPetriNet<'analysis, 'tcx> {
    pub fn new(
        options: Options,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        callgraph: &'analysis CallGraph<'tcx>,
        unsafe_data: UnsafeData,
        av: bool,
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, &callgraph, av));
        ColorPetriNet {
            options,
            tcx,
            net: DiGraph::new(),
            callgraph,
            alias,
            function_counter: HashMap::new(),
            marking: HashMap::new(),
            entry_node: NodeIndex::new(0),
            unsafe_data,
            unsafe_places: HashMap::new(),
        }
    }

    // 添加控制库所
    fn add_control_place(&mut self, basic_block: String, token_num: usize) -> NodeIndex {
        self.net.add_node(ColorPetriNode::ControlPlace {
            basic_block,
            token_num: Arc::new(RwLock::new(token_num)),
        })
    }

    fn add_temp_data_place(
        &mut self,
        basic_block: String,
        local: usize,
        token_num: usize,
    ) -> NodeIndex {
        self.net.add_node(ColorPetriNode::TempDataPlace {
            basic_block,
            local,
            token_num: Arc::new(RwLock::new(token_num)),
        })
    }

    // 添加边
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: u32) {
        self.net.add_edge(from, to, ColorPetriEdge { weight });
    }

    pub fn construct_unsafe_places(&mut self) {
        let mut next_alias_id: u32 = 0;
        let mut alias_groups: HashMap<u32, Vec<(AliasId, String)>> = HashMap::new();
        let places_data: Vec<_> = self
            .unsafe_data
            .unsafe_places
            .iter()
            .map(|(local, info)| (*local, info.clone()))
            .collect();

        // 两两比较数据
        for i in 0..places_data.len() {
            let (local_i, info_i) = &places_data[i];

            // 如果这个数据已经被分配了别名组，跳过
            if alias_groups
                .values()
                .any(|group| group.iter().any(|(l, _)| l == local_i))
            {
                continue;
            }

            let mut current_group = vec![(local_i.clone(), info_i.clone())];

            // 与剩余数据比较
            for j in i + 1..places_data.len() {
                let (local_j, info_j) = &places_data[j];
                match self.alias.borrow_mut().alias(*local_i, *local_j) {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                        current_group.push((local_j.clone(), info_j.clone()));
                    }
                    _ => {}
                }
            }

            // 如果找到了别名（包括自己），创建新的别名组
            if !current_group.is_empty() {
                alias_groups.insert(next_alias_id, current_group);
                next_alias_id += 1;
            }
        }

        // 为每个别名组创建数据库所
        for (_, group) in alias_groups {
            let unsafe_span = group[0].1.clone();
            let unsafe_local = group[0].0.clone();
            let node = self.add_temp_data_place(
                unsafe_span,
                unsafe_local.local.index(),
                // group.len() as u8,
                1,
            );

            // 记录每个数据对应的库所
            for (local, _) in group {
                self.unsafe_places.insert(local, node);
            }
        }
        // log::info!("unsafe_places: {:?}", self.unsafe_places);
    }

    // 设置标识
    pub fn set_marking(&mut self, place: NodeIndex, tokens: usize) {
        self.marking.insert(place, tokens);
    }

    pub fn get_marking(&self) -> HashSet<(NodeIndex, usize)> {
        let mut current_mark = HashSet::<(NodeIndex, usize)>::new();
        for node in self.net.node_indices() {
            match &self.net[node] {
                ColorPetriNode::ControlPlace { token_num, .. }
                | ColorPetriNode::TempDataPlace { token_num, .. } => {
                    if *token_num.read().unwrap() > 0 {
                        current_mark.insert((node.clone(), *token_num.read().unwrap() as usize));
                    }
                }
                _ => {}
            }
        }
        current_mark
    }

    pub fn construct(&mut self /*alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>*/) {
        self.construct_func();
        self.construct_unsafe_places();

        // 设置一个id,记录已经转换的函数
        let mut visited_func_id = HashSet::<DefId>::new();
        for (node, caller) in self.callgraph.graph.node_references() {
            if self.tcx.is_mir_available(caller.instance().def_id())
                && format_name(caller.instance().def_id()).starts_with(&self.options.crate_name)
            {
                log::debug!(
                    "visitor function body: {:?}",
                    format_name(caller.instance().def_id())
                );
                if visited_func_id.contains(&caller.instance().def_id()) {
                    continue;
                }

                self.visitor_instance_body(node, caller);
                visited_func_id.insert(caller.instance().def_id());
            }
        }

        self.reduce_state();

        // 验证网络结构
        if let Err(err) = self.verify_structure() {
            log::error!("Color Petri net structure verification failed: {}", err);
            // 可以选择在这里panic或进行其他错误处理
        }
    }

    pub fn cpn_to_dot(&self, filename: &str) -> std::io::Result<()> {
        use petgraph::dot::{Config, Dot};
        use std::fs::File;
        use std::io::Write;

        let dot = Dot::with_attr_getters(
            &self.net,
            &[Config::NodeNoLabel],
            &|_, e| "arrowhead = vee".to_string(),
            &|_, n| match &n.1 {
                ColorPetriNode::DataPlace {
                    token_type,
                    token_num,
                } => {
                    let type_labels: Vec<String> = token_type
                        .iter()
                        .map(|t| format!("{:?}:{}", t.local, t.ty))
                        .collect();
                    format!(
                            "label = \"DP\\n{}\", shape = circle, style = filled, fillcolor = lightblue",
                            type_labels.join("\\n")
                        )
                }
                ColorPetriNode::TempDataPlace {
                    basic_block,
                    local,
                    token_num,
                } => {
                    format!(
                        "label = \"UDP\\n{} and {}\", shape = circle, style = filled, fillcolor = red",
                        basic_block,
                        local
                    )
                }
                ColorPetriNode::ControlPlace {
                    basic_block,
                    token_num,
                } => {
                    format!(
                            "label = \"Control Place\\n{}\", shape = circle, style = filled, fillcolor = lightgreen",
                            basic_block
                        )
                }
                ColorPetriNode::UnsafeTransition {
                    data_ops,
                    info,
                    span,
                    rw_type,
                    basic_block,
                } => {
                    // let ops: Vec<String> = data_ops
                    //     .iter()
                    //     .map(|op| format!("{:?} {:?}", op.op_type, op.data.local))
                    //     .collect();
                    // format!(
                    //         "label = \"Unsafe Trans\\n{}\", shape = box, style = filled, fillcolor = pink",
                    //         ops.join("\\n")
                    //     )
                    format!(
                        "label = \"Unsafe Trans\\n{:?}\", shape = box, style = filled, fillcolor = pink",
                        data_ops
                    )
                }
                ColorPetriNode::Cfg { name } => {
                    format!(
                        "label = \"CFG\\n{}\", shape = box, style = filled, fillcolor = lightgreen",
                        name
                    )
                }
            },
        );

        let mut file = File::create(filename)?;
        writeln!(file, "{:?}", dot)?;
        Ok(())
    }

    pub fn visitor_instance_body(&mut self, node: NodeIndex, caller: &CallGraphNode<'tcx>) {
        let body = self.tcx.optimized_mir(caller.instance().def_id());
        if body.source.promoted.is_some() {
            return;
        }

        // let mut func_body = BodyToColorPetriNet::new(node, caller.instance(), body, &mut self);

        let mut func_body = BodyToColorPetriNet::new(
            node,
            caller.instance(),
            body,
            self.tcx,
            &self.options,
            &self.callgraph,
            &mut self.net,
            &mut self.alias,
            &self.function_counter,
            &self.unsafe_data.unsafe_places,
            &self.unsafe_places,
        );
        func_body.translate();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        if let Some((main_func, _)) = self.tcx.entry_fn(()) {
            for node_idx in self.callgraph.graph.node_indices() {
                let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
                let func_id = func_instance.instance().def_id();
                let func_name = format_name(func_id);
                if !func_name.starts_with(&self.options.crate_name) {
                    continue;
                }

                // 检查是否已经存在
                if self.function_counter.contains_key(&func_id) {
                    continue;
                }

                let func_start = format!("{}_start", func_name);
                let func_end = format!("{}_end", func_name);
                let (func_start_node_id, func_end_node_id) = if func_id == main_func {
                    let func_start_node_id = self.add_control_place(func_start.clone(), 1);
                    let func_end_node_id = self.add_control_place(func_end.clone(), 0);
                    self.entry_node = func_start_node_id;
                    (func_start_node_id, func_end_node_id)
                } else {
                    let func_start_node_id = self.add_control_place(func_start.clone(), 0);
                    let func_end_node_id = self.add_control_place(func_end.clone(), 0);
                    (func_start_node_id, func_end_node_id)
                };

                self.function_counter
                    .insert(func_id, (func_start_node_id, func_end_node_id));
            }
        } else {
            log::debug!("cargo pta need a entry point!");
        }
    }

    /// 验证彩色Petri网的结构正确性
    ///
    /// 检查规则:
    /// 1. UnsafeTransition的前驱必须包含至少一个控制库所和一个数据库所
    /// 2. UnsafeTransition的后继必须包含至少一个控制库所
    /// 3. Cfg变迁的前驱和后继只能是ControlPlace
    /// 4. 所有库所(DataPlace/TempDataPlace/ControlPlace)的前驱和后继只能是变迁
    pub fn verify_structure(&self) -> anyhow::Result<()> {
        use petgraph::Direction;

        for node_idx in self.net.node_indices() {
            match &self.net[node_idx] {
                ColorPetriNode::UnsafeTransition { info, span, .. } => {
                    // 检查前驱
                    let mut has_control_pred = false;
                    let mut has_data_pred = false;

                    for pred in self.net.neighbors_directed(node_idx, Direction::Incoming) {
                        match &self.net[pred] {
                            ColorPetriNode::ControlPlace { .. } => has_control_pred = true,
                            ColorPetriNode::DataPlace { .. }
                            | ColorPetriNode::TempDataPlace { .. } => has_data_pred = true,
                            other => {
                                return Err(
                                    CpnStructureError::UnsafeTransitionInvalidPredecessor {
                                        span: span.clone(),
                                        found_type: format!("{:?}", other),
                                    }
                                    .into(),
                                );
                            }
                        }
                    }

                    if !has_control_pred || !has_data_pred {
                        return Err(CpnStructureError::UnsafeTransitionMissingPredecessors {
                            span: span.clone(),
                            has_control: has_control_pred,
                            has_data: has_data_pred,
                        }
                        .into());
                    }

                    // 检查后继
                    let mut has_control_succ = false;
                    for succ in self.net.neighbors_directed(node_idx, Direction::Outgoing) {
                        match &self.net[succ] {
                            ColorPetriNode::ControlPlace { .. }
                            | ColorPetriNode::TempDataPlace { .. }
                            | ColorPetriNode::DataPlace { .. } => has_control_succ = true,
                            other => {
                                return Err(CpnStructureError::UnsafeTransitionInvalidSuccessor {
                                    span: span.clone(),
                                    found_type: format!("{:?}", other),
                                }
                                .into());
                            }
                        }
                    }

                    if !has_control_succ {
                        return Err(CpnStructureError::UnsafeTransitionMissingControlSuccessor {
                            span: span.clone(),
                        }
                        .into());
                    }
                }

                ColorPetriNode::Cfg { name } => {
                    for pred in self.net.neighbors_directed(node_idx, Direction::Incoming) {
                        if !matches!(&self.net[pred], ColorPetriNode::ControlPlace { .. }) {
                            return Err(CpnStructureError::CfgInvalidPredecessor {
                                name: name.clone(),
                                found_type: format!("{:?}", &self.net[pred]),
                            }
                            .into());
                        }
                    }

                    for succ in self.net.neighbors_directed(node_idx, Direction::Outgoing) {
                        if !matches!(&self.net[succ], ColorPetriNode::ControlPlace { .. }) {
                            return Err(CpnStructureError::CfgInvalidSuccessor {
                                name: name.clone(),
                                found_type: format!("{:?}", &self.net[succ]),
                            }
                            .into());
                        }
                    }
                }
                ColorPetriNode::ControlPlace { basic_block, .. }
                | ColorPetriNode::TempDataPlace { basic_block, .. } => {
                    for pred in self.net.neighbors_directed(node_idx, Direction::Incoming) {
                        if !matches!(
                            &self.net[pred],
                            ColorPetriNode::UnsafeTransition { .. } | ColorPetriNode::Cfg { .. }
                        ) {
                            return Err(CpnStructureError::PlaceInvalidPredecessor {
                                place_type: "ControlPlace".to_string(),
                                basic_block: basic_block.clone(),
                                found_type: format!("{:?}", &self.net[pred]),
                            }
                            .into());
                        }
                    }

                    for succ in self.net.neighbors_directed(node_idx, Direction::Outgoing) {
                        if !matches!(
                            &self.net[succ],
                            ColorPetriNode::UnsafeTransition { .. } | ColorPetriNode::Cfg { .. }
                        ) {
                            return Err(CpnStructureError::PlaceInvalidSuccessor {
                                place_type: "ControlPlace".to_string(),
                                basic_block: basic_block.clone(),
                                found_type: format!("{:?}", &self.net[succ]),
                            }
                            .into());
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 简化彩色Petri网中的状态，通过合并简单路径来减少网络的复杂度
    ///
    /// 具体步骤:
    /// 1. 找到所有入度和出度都≤1的控制库所作为起始点
    /// 2. 从每个起始点开始，向两个方向搜索，找到可以合并的路径
    /// 3. 对于每条找到的路径:
    ///    - 确保路径的起点和终点都是控制库所
    ///    - 如果路径长度>3，则创建一个新的Cfg变迁来替代中间的节点
    ///    - 保持路径两端的控制库所不变，删除中间的所有节点
    /// 4. 最后统一删除所有被标记为需要移除的节点
    pub fn reduce_state(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut all_nodes_to_remove = Vec::new();

        // 找到所有入度和出度都≤1的控制库所
        for node in self.net.node_indices() {
            if let ColorPetriNode::ControlPlace { .. } = &self.net[node] {
                let in_degree = self.net.edges_directed(node, Direction::Incoming).count();
                let out_degree = self.net.edges_directed(node, Direction::Outgoing).count();

                if in_degree <= 1 && out_degree <= 1 {
                    queue.push_back(node);
                }
            }
        }

        while let Some(start) = queue.pop_front() {
            if visited.contains(&start) {
                continue;
            }

            // 从start开始BFS，找到一条链
            let mut chain = vec![start];
            let mut current = start;
            visited.insert(start);

            // 向两个方向遍历
            for direction in &[Direction::Outgoing, Direction::Incoming] {
                current = start;
                loop {
                    let neighbors: Vec<_> =
                        self.net.neighbors_directed(current, *direction).collect();

                    if neighbors.len() != 1 {
                        break;
                    }

                    let next = neighbors[0];
                    let next_in_degree = self.net.edges_directed(next, Direction::Incoming).count();
                    let next_out_degree =
                        self.net.edges_directed(next, Direction::Outgoing).count();

                    // 只处理简单的控制流路径
                    match &self.net[next] {
                        ColorPetriNode::ControlPlace { .. } | ColorPetriNode::Cfg { .. } => {
                            if next_in_degree > 1 || next_out_degree > 1 || visited.contains(&next)
                            {
                                break;
                            }
                            visited.insert(next);
                            if *direction == Direction::Outgoing {
                                chain.push(next);
                            } else {
                                chain.insert(0, next);
                            }
                            current = next;
                        }
                        // 不处理包含数据操作的路径
                        _ => break,
                    }
                }
            }

            // 调整链，确保起始和结束都是ControlPlace
            if !chain.is_empty() {
                if !matches!(&self.net[chain[0]], ColorPetriNode::ControlPlace { .. }) {
                    chain.remove(0);
                }
            }
            if !chain.is_empty() {
                if !matches!(
                    &self.net[chain[chain.len() - 1]],
                    ColorPetriNode::ControlPlace { .. }
                ) {
                    chain.pop();
                }
            }

            // 检查调整后的链长度是否满足简化条件
            if chain.len() > 3 {
                let p1 = chain[0];
                let p2 = chain[chain.len() - 1];

                // 确保p1和p2都是ControlPlace
                if let (
                    ColorPetriNode::ControlPlace {
                        basic_block: bb1, ..
                    },
                    ColorPetriNode::ControlPlace {
                        basic_block: bb2, ..
                    },
                ) = (&self.net[p1], &self.net[p2])
                {
                    // 创建新的Cfg变迁
                    let new_cfg = ColorPetriNode::Cfg {
                        name: format!("merged_cfg_{}_to_{}", bb1, bb2),
                    };
                    let new_cfg_idx = self.net.add_node(new_cfg);

                    // 添加新边
                    self.add_edge(p1, new_cfg_idx, 1);
                    self.add_edge(new_cfg_idx, p2, 1);

                    // 收集要删除的节点
                    all_nodes_to_remove.extend(chain[1..chain.len() - 1].iter().cloned());
                }
            }
        }

        // 在循环结束后统一删除节点
        if !all_nodes_to_remove.is_empty() {
            // 按索引从大到小排序
            all_nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
            // 删除节点
            for node in all_nodes_to_remove {
                self.net.remove_node(node);
            }
        }
    }
}
