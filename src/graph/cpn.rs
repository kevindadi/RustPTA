use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::IntoNodeReferences,
};
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Local};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    analysis::pointsto::{AliasAnalysis, AliasId, ApproximateAliasKind},
    memory::unsafe_memory::{UnsafeData, UnsafeDataInfo, UnsafeInfo},
    options::Options,
    utils::format_name,
};

use super::{
    callgraph::{CallGraph, CallGraphNode, InstanceId},
    mir_cpn::BodyToColorPetriNet,
};

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
    pub(crate) op_type: DataOpType,
    // 操作的数据
    pub(crate) data: DataTokenType,
    // 操作的线程
    pub(crate) thread_name: String,
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
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, &callgraph));
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
                            "label = \"Data Place\\n{}\", shape = circle, style = filled, fillcolor = lightblue",
                            type_labels.join("\\n")
                        )
                }
                ColorPetriNode::TempDataPlace {
                    basic_block,
                    local,
                    token_num,
                } => {
                    format!(
                        "label = \"Unsafe Data Place\\n{} and {}\", shape = circle, style = filled, fillcolor = red",
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
}
