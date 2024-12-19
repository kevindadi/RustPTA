use petgraph::dot::{Config, Dot};
use petgraph::graph::{node_index, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use serde_json::json;

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::hash::Hasher;

use crate::concurrency::atomic::AtomicOrdering;
use crate::memory::pointsto::AliasId;

use super::pn::{ControlType, PetriNetEdge, PetriNetNode, PlaceType, TransitionType};

use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Serialize)]
struct AtomicViolation {
    violation_type: String,
    variable: String,
    locations: Vec<Location>,
    state: Option<Vec<(usize, usize)>>, // 仅用于并发违背
}

#[derive(Serialize)]
struct Location {
    operation: String,
    span: String,
}
#[derive(Debug, Clone)]
pub struct StateEdge {
    pub label: String,
    pub weight: u32,
}

impl StateEdge {
    pub fn new(label: String, weight: u32) -> Self {
        Self { label, weight }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateNode {
    pub mark: Vec<(usize, usize)>,
}

impl Hash for StateNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut sorted_mark = self.mark.clone();
        sorted_mark.sort(); // 确保排序后再计算哈希值
        sorted_mark.hash(state);
    }
}

impl Eq for StateNode {}

impl StateNode {
    pub fn new(mark: Vec<(usize, usize)>) -> Self {
        Self { mark }
    }
}

// 规范化状态表示
pub fn normalize_state(mark: &HashSet<(NodeIndex, usize)>) -> Vec<(usize, usize)> {
    let mut state: Vec<(usize, usize)> = mark.iter().map(|(n, t)| (n.index(), *t)).collect();
    state.sort();
    state
}

pub fn insert_with_comparison(
    set: &mut HashSet<Vec<(usize, usize)>>,
    value: &Vec<(usize, usize)>,
) -> bool {
    for existing_value in set.iter() {
        if existing_value == value {
            return false;
        }
    }
    set.insert(value.clone());
    return true;
}

#[derive(Debug, Clone)]
pub struct StateGraph {
    pub graph: Graph<StateNode, StateEdge>,
    initial_net: Box<Graph<PetriNetNode, PetriNetEdge>>,
    initial_mark: HashSet<(NodeIndex, usize)>,
    deadlock_marks: HashSet<Vec<(usize, usize)>>,
    pub apis_deadlock_marks: HashMap<String, HashSet<Vec<(usize, usize)>>>,
    apis_graph: HashMap<String, Box<Graph<StateNode, StateEdge>>>,
}

impl StateGraph {
    pub fn new(
        initial_net: Graph<PetriNetNode, PetriNetEdge>,
        initial_mark: HashSet<(NodeIndex, usize)>,
    ) -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
            initial_net: Box::new(initial_net),
            initial_mark,
            deadlock_marks: HashSet::new(),
            apis_deadlock_marks: HashMap::new(),
            apis_graph: HashMap::new(),
        }
    }

    /// 生成 Petri 网从初始状态可达的所有状态
    ///
    /// 该函数使用广度优先搜索和并行处理的方式来探索所有可达状态。
    /// 对于每个状态，计算其使能的变迁，并行地发生这些变迁以生成新状态，
    /// 如果生成的新状态是唯一的，则将其添到状态图中。
    pub fn generate_states(&mut self) {
        let mut queue = VecDeque::new();
        let all_states = Arc::new(Mutex::new(HashSet::<Vec<(usize, usize)>>::new()));
        let mut visited_states = HashSet::new();
        // 初始状态队列，加入初始网和标识
        queue.push_back((self.initial_net.clone(), self.initial_mark.clone()));
        {
            all_states
                .lock()
                .unwrap()
                .insert(normalize_state(&self.initial_mark));
        }
        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            // 获取当前状态下所有使能的变迁

            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);

            // 如果没有使能的变迁，将当前状态添加到死锁标识集合中
            if enabled_transitions.is_empty() {
                let current_state_normalized = normalize_state(&current_mark);
                self.deadlock_marks.insert(current_state_normalized.clone());
                continue;
            }

            let current_state = normalize_state(&current_mark);
            if !visited_states.insert(current_state.clone()) {
                continue; // 跳过已访问的状态
            }
            let current_node = self.graph.add_node(StateNode::new(current_state.clone()));
            // 并行处理每个变迁，生成新状态，同时保存变迁信息
            // let new_states: Vec<_> = enabled_transitions
            //     .into_par_iter()
            //     .map(|transition| {
            //         let mut net_clone = current_net.clone();
            //         let (new_net, new_mark) =
            //             self.fire_transition(&mut net_clone, &current_mark, transition);
            //         (transition, new_net, new_mark)
            //     })
            //     .collect();

            // 使用 std::thread 替换 rayon
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
                    let new_node = self.graph.add_node(StateNode::new(new_state));

                    // 添加从当前状态到新状态的边，边的标签为变迁名
                    self.graph.add_edge(
                        current_node,
                        new_node,
                        StateEdge::new(format!("{:?}", transition), 1),
                    );
                }
            }
        }
    }

    pub fn generate_states_with_api(
        &mut self,
        api_name: String,
        api_initial_mark: HashSet<(NodeIndex, usize)>,
    ) {
        let mut queue = VecDeque::new();
        let all_states = Arc::new(Mutex::new(HashSet::<Vec<(usize, usize)>>::new()));
        let mut visited_states = HashSet::new();
        // 初始化状态队列，加入初始网和标识
        queue.push_back((self.initial_net.clone(), api_initial_mark.clone()));
        {
            all_states
                .lock()
                .unwrap()
                .insert(normalize_state(&api_initial_mark));
        }
        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);

            if enabled_transitions.is_empty() {
                let current_state_normalized = normalize_state(&current_mark);
                self.apis_deadlock_marks
                    .entry(api_name.clone())
                    .or_insert(HashSet::new())
                    .insert(current_state_normalized.clone());
                continue;
            }

            let current_state = normalize_state(&current_mark);
            if !visited_states.insert(current_state.clone()) {
                continue; // 跳过已访问的状态
            }
            let current_node = self
                .apis_graph
                .entry(api_name.clone())
                .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                .add_node(StateNode::new(current_state.clone()));
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
                let mut all_states_guard = all_states.lock().unwrap();
                if insert_with_comparison(&mut all_states_guard, &new_state) {
                    queue.push_back((new_net.clone(), new_mark.clone()));
                    let new_node = self
                        .apis_graph
                        .entry(api_name.clone())
                        .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                        .add_node(StateNode::new(new_state));

                    self.apis_graph
                        .entry(api_name.clone())
                        .or_insert(Box::new(Graph::<StateNode, StateEdge>::new()))
                        .add_edge(
                            current_node,
                            new_node,
                            StateEdge::new(format!("{:?}", transition), 1),
                        );
                }
            }
        }
    }

    #[inline]
    fn set_current_mark(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) {
        // 首先将所有库所的 token 清零
        for node_index in net.node_indices() {
            if let Some(PetriNetNode::P(place)) = net.node_weight(node_index) {
                *place.tokens.write().unwrap() = 0;
            }
        }

        // 直接根据 mark 中的 NodeIndex 设置对应的 token
        for (node_index, token_count) in mark {
            if let Some(PetriNetNode::P(place)) = net.node_weight(*node_index) {
                // let tokens = *place.tokens.write().unwrap();
                {
                    *place.tokens.write().unwrap() = *token_count;
                }
                assert!(
                    *place.tokens.read().unwrap() <= place.capacity,
                    "Token count ({}) exceeds capacity ({}) at node index {}, and token_count is {} ",
                    *place.tokens.read().unwrap(),
                    place.capacity,
                    node_index.index(),
                    token_count
                );
            }
        }
    }

    /// 获取当前标识下所有使能的变迁
    /// 1. 使用 `set_current_mark` 函数设置当前标识
    /// 2. 遍历网络中的每个节点，检查其是否为变迁节点
    /// 3. 对于每个变迁节点，检查其所有输入库所是否有足够的 token
    /// 4. 如果所有输入库所的 token 数量均满足要求，则该变迁为使能状态
    /// 5. 将所有使能的变迁节点索引添加到返回的向量中
    fn get_enabled_transitions(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();

        // 使用内联函数设置当前标识
        self.set_current_mark(net, mark);

        // 检查变迁使能的逻辑
        for node_index in net.node_indices() {
            match net.node_weight(node_index) {
                Some(PetriNetNode::T(_)) => {
                    let mut enabled = true;
                    for edge in net.edges_directed(node_index, Direction::Incoming) {
                        match net.node_weight(edge.source()).unwrap() {
                            PetriNetNode::P(place) => {
                                if *place.tokens.read().unwrap() < edge.weight().label {
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

    /// 发生一个变迁并生成新的网络状态
    /// 1. 克隆当前网络创建新图
    /// 2. 根据当前标识设置初始 token
    /// 3. 从变迁的输入库所中减去相应的 token
    /// 4. 向变迁的输出库所中添加相应的 token（考虑容量限制）
    /// 5. 生成并返回新的状态
    fn fire_transition(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
        transition: NodeIndex,
    ) -> (
        Box<Graph<PetriNetNode, PetriNetEdge>>,
        HashSet<(NodeIndex, usize)>,
    ) {
        let mut new_net = net.clone(); // 克隆当前网，创建新图
        self.set_current_mark(&mut new_net, mark);
        let mut new_state = HashSet::<(NodeIndex, usize)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        // 从输入库所中减去token
        log::debug!("sub token to source node!");
        for edge in new_net.edges_directed(transition, Direction::Incoming) {
            match new_net.node_weight(edge.source()).unwrap() {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens -= edge.weight().label;
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        // 将token添加到输出库所中
        log::debug!("add token to target node!");
        for edge in new_net.edges_directed(transition, Direction::Outgoing) {
            let place_node = new_net.node_weight(edge.target()).unwrap();
            match place_node {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens += edge.weight().label;
                    if *tokens > place.capacity {
                        *tokens = place.capacity;
                    }
                    assert!(place.capacity > 0);
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        log::debug!("generate new state!");
        for node in new_net.node_indices() {
            match &new_net[node] {
                PetriNetNode::P(place) => {
                    let tokens = *place.tokens.read().unwrap();
                    if tokens > 0 {
                        // 确保token数量不超过容量限制
                        let final_tokens = tokens.min(place.capacity);
                        new_state.insert((node, final_tokens));
                    }
                }
                PetriNetNode::T(_) => {}
            }
        }

        (Box::new(new_net), new_state) // 返回新图和新状态
    }

    // Check Deadlock
    pub fn check_deadlock(&mut self) -> String {
        use petgraph::graph::node_index;
        // Remove the terminal mark
        self.deadlock_marks.retain(|v| {
            v.iter().all(|m| match &self.initial_net[node_index(m.0)] {
                PetriNetNode::P(p) => !p.name.contains("mainend"),
                _ => false,
            })
        });

        if self.deadlock_marks.is_empty() {
            return "No deadlock detected.\n".to_string();
        }

        let mut result = String::from("Detected deadlock states:\n");
        for (i, mark) in self.deadlock_marks.iter().enumerate() {
            result.push_str(&format!("\nDeadlock State #{}\n", i + 1));
            result.push_str("Active Places:\n");

            let places: Vec<String> = mark
                .iter()
                .filter_map(|x| match &self.initial_net[node_index(x.0)] {
                    PetriNetNode::P(p) => Some(format!(
                        "  - {} (tokens: {}, location: {})",
                        p.name, x.1, p.span
                    )),
                    _ => None,
                })
                .collect();

            result.push_str(&places.join("\n"));
            result.push('\n');
        }

        result
    }

    pub fn check_api_deadlock(&mut self) -> String {
        let mut result = String::from("\n");
        for (api_name, deadlock_marks) in self.apis_deadlock_marks.iter() {
            result.push_str(&format!("API: {}\n", api_name));
            if deadlock_marks.is_empty() {
                result.push_str(&format!("No deadlock detected.\n"));
            } else {
                result.push_str(&format!("Detected deadlock states:\n"));
                for (i, mark) in deadlock_marks.iter().enumerate() {
                    result.push_str(&format!("Deadlock State #{}:\n", i + 1));
                    result.push_str("Active Places:\n");

                    let places: Vec<String> = mark
                        .iter()
                        .filter_map(|x| match &self.initial_net[node_index(x.0)] {
                            PetriNetNode::P(p) => Some(format!(
                                "  - {} (tokens: {}, location: {})",
                                p.name, x.1, p.span
                            )),
                            _ => None,
                        })
                        .collect();

                    result.push_str(&places.join("\n"));
                    result.push('\n');
                }
            }
        }
        result
    }

    pub fn check_atomic_violation(&mut self) -> String {
        let mut violations = Vec::new();

        for node_idx in self.graph.node_indices() {
            if let Some(PetriNetNode::T(transition)) = self.initial_net.node_weight(node_idx) {
                if let TransitionType::AtomicLoad(var_id, _, load_span) =
                    &transition.transition_type
                {
                    let (forward_violation, forward_stores) =
                        self.check_path_violation(node_idx, var_id, Direction::Incoming);
                    let (backward_violation, backward_stores) =
                        self.check_path_violation(node_idx, var_id, Direction::Outgoing);

                    if forward_violation || backward_violation {
                        let mut locations = vec![Location {
                            operation: "load".to_string(),
                            span: load_span.clone(),
                        }];

                        // 添加前向路径上的 store 操作
                        for (store_span, direction) in forward_stores {
                            locations.push(Location {
                                operation: format!("store_{}", direction),
                                span: store_span,
                            });
                        }

                        // 添加后向路径上的 store 操作
                        for (store_span, direction) in backward_stores {
                            locations.push(Location {
                                operation: format!("store_{}", direction),
                                span: store_span,
                            });
                        }

                        violations.push(AtomicViolation {
                            violation_type: "unsynchronized_path".to_string(),
                            variable: format!("{:?}", var_id),
                            locations,
                            state: None,
                        });
                    }
                }
            }
        }

        // 2. 检查状态图中的并发 Relaxed 操作
        for state_node in self.graph.node_indices() {
            let mut relaxed_ops = HashMap::new();

            // 收集当前状态下所有可能的 Relaxed 操作
            for edge in self.graph.edges(state_node) {
                if let Some(PetriNetNode::T(transition)) =
                    self.initial_net.node_weight(edge.target())
                {
                    match &transition.transition_type {
                        TransitionType::AtomicLoad(var_id, AtomicOrdering::Relaxed, span)
                        | TransitionType::AtomicStore(var_id, AtomicOrdering::Relaxed, span) => {
                            relaxed_ops
                                .entry(var_id.instance_id)
                                .or_insert(Vec::new())
                                .push((edge.target(), span.clone()));
                        }
                        _ => {}
                    }
                }
            }

            // 检查每个变量的并发操作
            for (var_id, ops) in relaxed_ops {
                if ops.len() >= 2 {
                    // 检查每对操作是否都缺乏同步机制
                    for i in 0..ops.len() {
                        for j in i + 1..ops.len() {
                            let (node1, span1) = &ops[i];
                            let (node2, span2) = &ops[j];

                            // 检查这两个操作的路径上是否有同步机制
                            if !self.has_sync_between_ops(*node1, *node2) {
                                let mut locations = Vec::new();
                                locations.push(Location {
                                    operation: "concurrent_relaxed".to_string(),
                                    span: span1.clone(),
                                });
                                locations.push(Location {
                                    operation: "concurrent_relaxed".to_string(),
                                    span: span2.clone(),
                                });

                                if let Some(state_mark) = self.graph.node_weight(state_node) {
                                    violations.push(AtomicViolation {
                                        violation_type: "concurrent_relaxed_operations".to_string(),
                                        variable: format!("NodeIndex({})", var_id.index()),
                                        locations,
                                        state: Some(state_mark.mark.clone()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        if violations.is_empty() {
            log::info!("No atomicity violations detected.");
        }

        log::info!(
            "Atomic violations:\n{}",
            serde_json::to_string_pretty(&json!({
                "violations": violations.iter().map(|v| {
                    json!({
                        "type": v.violation_type,
                        "variable": v.variable,
                        "locations": v.locations.iter().map(|loc| {
                            json!({
                                "operation": loc.operation,
                                "span": loc.span
                            })
                        }).collect::<Vec<_>>(),
                        "state": v.state
                    })
                }).collect::<Vec<_>>()
            }))
            .unwrap()
        );

        "".to_string()
    }

    // 修改返回类型以包含 store 的位置信息
    fn check_path_violation(
        &self,
        start: NodeIndex,
        var_id: &AliasId,
        direction: Direction,
    ) -> (bool, Vec<(String, String)>) {
        let mut visited = HashSet::new();
        let mut stack = vec![start];
        let mut violation_spans = Vec::new();
        let direction_str = match direction {
            Direction::Incoming => "before",
            Direction::Outgoing => "after",
        };

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }

            // 打印当前节点的所有邻居
            log::debug!("Current node: {:?}", current);
            log::debug!("Neighbors in direction {:?}:", direction_str);
            for edge in self.initial_net.edges_directed(current, direction) {
                let neighbor = match direction {
                    Direction::Incoming => edge.source(),
                    Direction::Outgoing => edge.target(),
                };
                if let Some(node) = self.initial_net.node_weight(neighbor) {
                    log::debug!("  -> {:?}: {:?}", neighbor, node);
                }
            }

            match self.initial_net.node_weight(current) {
                Some(PetriNetNode::T(transition)) => match &transition.transition_type {
                    TransitionType::AtomicStore(v_id, AtomicOrdering::Relaxed, span) => {
                        log::debug!("Comparing var_ids: current={:?}, target={:?}", v_id, var_id);
                        // 只比较 instance_id，忽略 local 字段
                        if v_id.instance_id == var_id.instance_id {
                            log::debug!("Found matching store: {:?} at {:?}", span, current);
                            violation_spans.push((span.clone(), direction_str.to_string()));
                        }
                    }
                    TransitionType::Lock
                    | TransitionType::Unlock
                    | TransitionType::RwLockRead
                    | TransitionType::RwLockWrite => {
                        log::debug!("Found sync operation at {:?}, stopping path", current);
                        return (false, vec![]);
                    }
                    _ => {}
                },
                Some(PetriNetNode::P(place)) => {
                    if place.place_type == PlaceType::Atomic {
                        log::debug!("Skipping atomic place: {:?}", place);
                        continue;
                    }
                }
                None => continue,
            }

            for edge in self.initial_net.edges_directed(current, direction) {
                let next = match direction {
                    Direction::Incoming => edge.source(),
                    Direction::Outgoing => edge.target(),
                };
                stack.push(next);
            }
        }

        log::debug!(
            "Path check complete. Found violations: {:?}",
            violation_spans
        );
        (!violation_spans.is_empty(), violation_spans)
    }

    fn has_sync_between_ops(&self, op1: NodeIndex, op2: NodeIndex) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![op1];

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }

            if current == op2 {
                continue;
            }

            if let Some(PetriNetNode::T(transition)) = self.initial_net.node_weight(current) {
                match transition.transition_type {
                    TransitionType::Lock
                    | TransitionType::Unlock
                    | TransitionType::RwLockRead
                    | TransitionType::RwLockWrite => {
                        return true;
                    }
                    _ => {}
                }
            }

            // 继续搜索相邻节点
            for edge in self.initial_net.edges(current) {
                stack.push(edge.target());
            }
        }

        false
    }

    #[allow(dead_code)]
    pub fn dot(&self, path: Option<&str>) -> std::io::Result<()> {
        let dot_string = format!(
            "digraph {{\n{:?}\n}}",
            Dot::with_config(&self.graph, &[Config::GraphContentOnly])
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
}
