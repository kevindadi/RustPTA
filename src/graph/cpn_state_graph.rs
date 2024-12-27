use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use serde::Serialize;
use std::hash::Hash;

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hasher;

use crate::memory::pointsto::AliasId;

use super::cpn::{ColorPetriEdge, ColorPetriNode, DataOpType};
use super::state_graph::{insert_with_comparison, normalize_state, StateEdge, StateNode};

use std::sync::{Arc, Mutex};

/// 数据竞争信息
#[derive(Debug, Clone, Eq, Serialize)]
pub struct RaceInfo {
    pub transitions: Vec<usize>,   // 关联的Unsafe变迁
    pub unsafe_data: RaceDataInfo, // 改回单个数据
    pub span: HashSet<String>,     // 用于匹配的span
    pub span_str: Vec<String>,     // 输出字符串
}

impl RaceInfo {
    fn extract_span_key(span: &str) -> String {
        // 匹配形如 "src/main.rs:19:17" 的部分
        if let Some(idx) = span.find(": ") {
            let location = &span[..idx];
            if let Some(last_colon) = location.rfind(':') {
                if let Some(prev_colon) = location[..last_colon].rfind(':') {
                    return location[..prev_colon].to_string();
                }
            }
        }
        span.to_string()
    }
}

impl PartialEq for RaceInfo {
    fn eq(&self, other: &Self) -> bool {
        // 提取并比较关键部分
        let self_spans: HashSet<_> = self
            .span
            .iter()
            .map(|s| RaceInfo::extract_span_key(s))
            .collect();
        let other_spans: HashSet<_> = other
            .span
            .iter()
            .map(|s| RaceInfo::extract_span_key(s))
            .collect();

        self_spans == other_spans
    }
}

impl Hash for RaceInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // 只哈希关键部分
        let mut spans: Vec<_> = self
            .span
            .iter()
            .map(|s| RaceInfo::extract_span_key(s))
            .collect();
        spans.sort();
        for span in spans {
            span.hash(state);
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct RaceDataInfo {
    pub data_func: usize,
    pub data_local: usize,
}

impl RaceDataInfo {
    pub fn new(data_func: usize, data_local: usize) -> Self {
        Self {
            data_func,
            data_local,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CpnStateGraph {
    pub graph: Graph<StateNode, StateEdge>,
    pub initial_net: Box<Graph<ColorPetriNode, ColorPetriEdge>>,
    pub initial_mark: HashSet<(NodeIndex, usize)>,
    pub race_info: Arc<Mutex<HashSet<RaceInfo>>>,
}

impl CpnStateGraph {
    pub fn new(
        initial_net: Graph<ColorPetriNode, ColorPetriEdge>,
        initial_mark: HashSet<(NodeIndex, usize)>,
    ) -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
            initial_net: Box::new(initial_net),
            initial_mark,
            race_info: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn insert_race_info(&mut self, race_info: RaceInfo) {
        self.race_info.lock().unwrap().insert(race_info);
    }

    pub fn generate_states(&mut self) {
        let mut queue = VecDeque::new();
        let all_states = Arc::new(Mutex::new(HashSet::<Vec<(usize, usize)>>::new()));
        let mut visited_states = HashSet::new();
        // 初始化状态队列，加入初始网和标识
        queue.push_back((self.initial_net.clone(), self.initial_mark.clone()));
        {
            all_states
                .lock()
                .unwrap()
                .insert(normalize_state(&self.initial_mark));
        }
        let mut race_handles = vec![];
        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            // 获取当前状态下所有使能的变迁

            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);
            let race_infos = self.detect_race_condition(&enabled_transitions);

            for race_info in race_infos {
                let race_info_clone = race_info.clone();
                let race_info_arc = Arc::clone(&self.race_info);

                let handle = std::thread::spawn(move || {
                    let mut guard = race_info_arc.lock().unwrap();
                    guard.insert(race_info_clone.clone());
                });

                race_handles.push(handle);
            }

            // 如果没有使能的变迁，将当前状态添加到死锁标识集合中
            if enabled_transitions.is_empty() {
                continue;
            }

            let mark_node_index = current_mark.clone().into_iter().map(|(n, _)| n).collect();
            let current_state = normalize_state(&current_mark);
            if !visited_states.insert(current_state.clone()) {
                continue; // 跳过已访问的状态
            }
            let current_node = self
                .graph
                .add_node(StateNode::new(current_state.clone(), mark_node_index));
            let new_states: Vec<_> = {
                let mut handles = vec![];

                for transition in enabled_transitions {
                    let current_net = current_net.clone();
                    let current_mark = current_mark.clone();
                    let self_clone = self.clone();

                    let handle = std::thread::spawn(move || {
                        let mut net_clone = current_net.clone();
                        let (new_net, new_mark) =
                            self_clone.fire_transition(&mut net_clone, &current_mark, transition);
                        (transition, new_net, new_mark)
                    });

                    handles.push(handle);
                }

                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect()
            };

            // 处理每个新生成的状态
            for (transition, new_net, new_mark) in new_states {
                let new_state = normalize_state(&new_mark);
                // std::thread::sleep(std::time::Duration::from_millis(500));
                // 检查新状态是否唯一，如果是则添加到状态图中
                let mut all_states_guard = all_states.lock().unwrap();
                if insert_with_comparison(&mut all_states_guard, &new_state) {
                    // if all_states_guard.insert(new_state.clone()) {
                    // 将新状态加入队列，等待后续处理
                    queue.push_back((new_net.clone(), new_mark.clone()));
                    // log::info!("new state: {:?}", new_state);
                    // 在状态图中添加新状态节点
                    let new_node = self.graph.add_node(StateNode::new(
                        new_state.clone(),
                        new_mark.clone().into_iter().map(|(n, _)| n).collect(),
                    ));

                    // 添加从当前状态到新状态的边，边的标签为变迁名
                    self.graph.add_edge(
                        current_node,
                        new_node,
                        StateEdge::new(format!("{:?}", transition), transition, 1),
                    );
                }
            }
        }
        for handle in race_handles {
            handle.join().unwrap();
        }
    }

    #[inline]
    fn set_current_mark(
        &self,
        net: &mut Graph<ColorPetriNode, ColorPetriEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) {
        // 首先将所有库所的 token 清零
        for node in net.node_indices() {
            match &net[node] {
                ColorPetriNode::ControlPlace { token_num, .. }
                | ColorPetriNode::TempDataPlace { token_num, .. } => {
                    *token_num.write().unwrap() = 0;
                }
                _ => {}
            }
        }

        // 直接根据 mark 中的 NodeIndex 设置对应的 token
        for (node_index, _) in mark {
            if let Some(ColorPetriNode::ControlPlace { token_num, .. })
            | Some(ColorPetriNode::TempDataPlace { token_num, .. }) =
                net.node_weight(*node_index)
            {
                // let tokens = *place.tokens.write().unwrap();
                {
                    *token_num.write().unwrap() = 1;
                }
            }
        }
    }

    fn get_enabled_transitions(
        &self,
        net: &mut Graph<ColorPetriNode, ColorPetriEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();

        // 使用内联函数设置当前标识
        self.set_current_mark(net, mark);

        // 检查变迁使能的逻辑
        for node_index in net.node_indices() {
            match net.node_weight(node_index) {
                Some(ColorPetriNode::UnsafeTransition { .. })
                | Some(ColorPetriNode::Cfg { .. }) => {
                    let mut enabled = true;
                    for edge in net.edges_directed(node_index, Direction::Incoming) {
                        match net.node_weight(edge.source()).unwrap() {
                            ColorPetriNode::ControlPlace { token_num, .. }
                            | ColorPetriNode::TempDataPlace { token_num, .. } => {
                                if *token_num.read().unwrap() == 0 {
                                    enabled = false;
                                    break;
                                }
                            }
                            _ => {
                                log::error!("The predecessor set of transition is not place");
                            }
                        }
                    }
                    if enabled {
                        sched_transiton.push(node_index);
                    }
                }
                _ => continue,
            }
        }

        sched_transiton
    }

    fn fire_transition(
        &self,
        net: &mut Graph<ColorPetriNode, ColorPetriEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
        transition: NodeIndex,
    ) -> (
        Box<Graph<ColorPetriNode, ColorPetriEdge>>,
        HashSet<(NodeIndex, usize)>,
    ) {
        let mut new_net = net.clone(); // 克隆当前网，创建新图
        self.set_current_mark(&mut new_net, mark);
        let mut new_state = HashSet::<(NodeIndex, usize)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        for edge in new_net.edges_directed(transition, Direction::Incoming) {
            match new_net.node_weight(edge.source()).unwrap() {
                ColorPetriNode::ControlPlace { token_num, .. }
                | ColorPetriNode::TempDataPlace { token_num, .. } => {
                    *token_num.write().unwrap() = 0;
                }
                _ => {
                    log::error!("Wrong Transition in NodeIndex:{}", edge.target().index());
                }
            }
        }

        for edge in new_net.edges_directed(transition, Direction::Outgoing) {
            let place_node = new_net.node_weight(edge.target()).unwrap();
            match place_node {
                ColorPetriNode::ControlPlace { token_num, .. }
                | ColorPetriNode::TempDataPlace { token_num, .. } => {
                    *token_num.write().unwrap() = 1;
                }
                _ => {
                    log::error!("Wrong Transition in NodeIndex:{}", edge.target().index());
                }
            }
        }

        for node in new_net.node_indices() {
            match &new_net[node] {
                ColorPetriNode::ControlPlace { token_num, .. }
                | ColorPetriNode::TempDataPlace { token_num, .. } => {
                    if *token_num.read().unwrap() > 0 {
                        new_state.insert((node, *token_num.read().unwrap()));
                    }
                }
                _ => {}
            }
        }
        log::debug!("Generate new state: {:?}", new_state);
        (Box::new(new_net), new_state) // 返回新图和新状态
    }

    #[allow(dead_code)]
    pub fn dot(&self, path: Option<&str>) -> std::io::Result<()> {
        let dot_string = format!(
            "digraph {{\n{:?}\n}}",
            Dot::with_config(&self.graph, &[Config::NodeNoLabel])
        );

        match path {
            Some(file_path) => {
                use std::fs::File;
                use std::io::Write;
                let mut file = File::create(file_path)?;
                file.write_all(dot_string.as_bytes())?;
                Ok(())
            }
            None => {
                println!("{}", dot_string);
                Ok(())
            }
        }
    }

    pub fn detect_race_condition(&self, enabled_transitions: &[NodeIndex]) -> HashSet<RaceInfo> {
        // 1. 收集所有UnsafeDataTransition，同时保存操作类型和span信息
        let mut data_groups: HashMap<AliasId, Vec<(NodeIndex, DataOpType, String, String, usize)>> =
            HashMap::new();
        for &trans_idx in enabled_transitions {
            if let ColorPetriNode::UnsafeTransition {
                ref data_ops,
                ref rw_type,
                ref info,
                ref span,
                basic_block,
            } = self.initial_net[trans_idx]
            {
                data_groups.entry(data_ops.clone()).or_default().push((
                    trans_idx,
                    rw_type.clone(),
                    info.clone(),
                    span.clone(),
                    basic_block,
                ));
            }
        }

        // 2. 检查每组数据访问是否存在竞争
        let mut race_infos = HashSet::new();
        for (data_ops, operations) in data_groups {
            if operations.len() < 2 {
                continue;
            }

            // 检查是否存在写操作
            let has_write = operations
                .iter()
                .any(|(_, op_type, _, _, _)| *op_type == DataOpType::Write);

            if !has_write {
                continue;
            }

            // // 收集所有相关的spans
            let mut span_str = Vec::new();
            for (_, op_type, info, _, _) in &operations {
                span_str.push(format!("({:?})-->{}", op_type, info));
            }

            let race_info = RaceInfo {
                transitions: operations.iter().map(|(t, _, _, _, _)| t.index()).collect(),
                unsafe_data: RaceDataInfo {
                    // TODO: 需要获取函数名
                    data_func: data_ops.instance_id.index(),
                    data_local: data_ops.local.index(),
                },
                span: operations
                    .iter()
                    .map(|(_, _, _, span, _)| span.clone())
                    .collect(),
                // rw_types: operations.iter().map(|(_, op, _, _)| op.clone()).collect(),
                // basic_blocks: operations.iter().map(|(_, _, _, bb)| bb.clone()).collect(),
                span_str: span_str,
            };

            race_infos.insert(race_info);
        }

        race_infos
    }
}
