use super::net_structure::{PetriNetEdge, PetriNetNode};
use crate::detect::atomicity_violation::AtomicOpType;
use crate::detect::atomicity_violation::AtomicRaceInfo;
use crate::detect::atomicity_violation::AtomicRaceOperation;
use crate::graph::net_structure::CallType;
use crate::graph::net_structure::ControlType;
use crate::options::Options;
use petgraph::dot::Dot;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use rustc_hir::def_id::DefId;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Write;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct StateEdge {
    pub label: String,
    pub transition: NodeIndex,
    pub transition_type: StateEdgeType,
    pub weight: u32,
}

#[derive(Debug, Clone)]
pub enum StateEdgeType {
    ControlFlow,
    ThreadOperate,
    LockOperate,
    AtomicOperate,
    UnsafeOperate,
}

impl Default for StateEdgeType {
    fn default() -> Self {
        StateEdgeType::ControlFlow
    }
}

impl StateEdge {
    pub fn new(label: String, transition: NodeIndex, weight: u32) -> Self {
        Self {
            label,
            transition,
            transition_type: StateEdgeType::default(),
            weight,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateNode {
    pub mark: Vec<(usize, u8)>,
    pub node_index: HashSet<NodeIndex>,
}

impl Hash for StateNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.mark.hash(state)
    }
}

impl PartialEq for StateNode {
    fn eq(&self, other: &Self) -> bool {
        self.mark == other.mark
    }
}

impl Eq for StateNode {}

impl StateNode {
    pub fn new(mark: Vec<(usize, u8)>, node_index: HashSet<NodeIndex>) -> Self {
        Self { mark, node_index }
    }
}

// 规范化状态表示
pub fn normalize_state(mark: &HashSet<(NodeIndex, u8)>) -> Vec<(usize, u8)> {
    let mut state: Vec<(usize, u8)> = mark.iter().map(|(n, t)| (n.index(), *t)).collect();
    state.sort();
    state
}

pub fn insert_with_comparison(
    set: &mut HashSet<Vec<(usize, u8)>>,
    value: &Vec<(usize, u8)>,
) -> bool {
    for existing_value in set.iter() {
        if existing_value == value {
            return false;
        }
    }
    set.insert(value.clone());
    return true;
}

#[derive(Debug, Serialize)]
pub struct DeadlockInfo {
    pub function_id: String,
    pub start_state: usize,
    pub deadlock_path: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct StateGraph {
    pub graph: Graph<StateNode, StateEdge>,
    pub initial_net: Rc<RefCell<Graph<PetriNetNode, PetriNetEdge>>>,
    pub initial_mark: HashSet<(NodeIndex, u8)>,
    pub deadlock_marks: HashSet<Vec<(usize, u8)>>,
    pub atomic_races: Vec<AtomicRaceInfo>,
    pub apis_deadlock_marks: HashMap<String, HashSet<Vec<(usize, u8)>>>,
    pub apis_graph: HashMap<String, Box<Graph<StateNode, StateEdge>>>,
    pub function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub options: Options,
    pub terminal_states: Vec<(usize, u8)>,
}

impl StateGraph {
    pub fn new(
        initial_net: Graph<PetriNetNode, PetriNetEdge>,
        initial_mark: HashSet<(NodeIndex, u8)>,
        function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
        options: Options,
        terminal_states: Vec<(usize, u8)>,
    ) -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
            initial_net: Rc::new(RefCell::new(initial_net)),
            initial_mark,
            deadlock_marks: HashSet::new(),
            atomic_races: Vec::new(),
            apis_deadlock_marks: HashMap::new(),
            apis_graph: HashMap::new(),
            function_counter,
            options,
            terminal_states,
        }
    }

    /// 生成 Petri 网从初始状态可达的所有状态
    ///
    /// 该函数使用广度优先搜索和并行处理的方式来探索所有可达状态。
    /// 对于每个状态，计算其使能的变迁，并行地发生这些变迁以生成新状态，
    /// 如果生成的新状态是唯一的，则将其添到状态图中。
    pub fn generate_states(&mut self) {
        let mut queue = VecDeque::new();
        let mut state_index_map = HashMap::<StateNode, NodeIndex>::new();
        let mut visited_states: HashSet<Vec<(usize, u8)>> = HashSet::new();

        queue.push_back(self.initial_mark.clone());
        let initial_state = StateNode::new(
            normalize_state(&self.initial_mark),
            self.initial_mark.iter().map(|(n, _)| *n).collect(),
        );

        let initial_node = self.graph.add_node(initial_state.clone());
        state_index_map.insert(initial_state.clone(), initial_node);

        while let Some(state_mark) = queue.pop_front() {
            let enabled_transitions = self.get_enabled_transitions(&state_mark);
            let current_state_normalized = normalize_state(&state_mark);
            // 检查死锁状态
            if enabled_transitions.is_empty() {
                self.deadlock_marks.insert(current_state_normalized.clone());
                continue;
            }

            let current_state_node = StateNode::new(
                current_state_normalized.clone(),
                state_mark.iter().map(|(n, _)| *n).collect(),
            );

            if visited_states.contains(&current_state_normalized) {
                continue;
            }
            visited_states.insert(current_state_normalized.clone());

            // 检查原子变量访问冲突
            if let Some(race_info) =
                self.check_atomic_race(&enabled_transitions, &normalize_state(&state_mark))
            {
                self.atomic_races.push(race_info);
            }

            let current_node = match state_index_map.get(&current_state_node) {
                Some(&node) => node,
                None => {
                    log::error!(
                        "Current state not found in index map: {:?}",
                        current_state_node
                    );
                    continue;
                }
            };

            // 串行处理每个使能的变迁
            for transition in enabled_transitions {
                match self.fire_transition(&state_mark, transition) {
                    Ok(new_mark) => {
                        let mark_node_index = new_mark.iter().map(|(n, _)| *n).collect();
                        let new_state = StateNode::new(normalize_state(&new_mark), mark_node_index);

                        // 处理新状态
                        if let Some(&existing_node) = state_index_map.get(&new_state) {
                            self.graph.add_edge(
                                current_node,
                                existing_node,
                                StateEdge::new(format!("{:?}", transition), transition, 1),
                            );
                        } else {
                            queue.push_back(new_mark);
                            let new_node = self.graph.add_node(new_state.clone());
                            state_index_map.insert(new_state, new_node);

                            self.graph.add_edge(
                                current_node,
                                new_node,
                                StateEdge::new(format!("{:?}", transition), transition, 1),
                            );
                        }
                    }
                    Err(e) => {
                        log::debug!("跳过无效变迁: {}", e);
                        continue;
                    }
                }
            }
        }
    }

    fn check_atomic_race(
        &mut self,
        enabled_transitions: &[NodeIndex],
        current_state: &Vec<(usize, u8)>,
    ) -> Option<AtomicRaceInfo> {
        let mut operations = Vec::new();
        let mut has_race = false;

        for &t in enabled_transitions {
            if let Some(PetriNetNode::T(transition)) = self.initial_net.borrow().node_weight(t) {
                match &transition.transition_type {
                    ControlType::Call(CallType::AtomicLoad(var_id, ordering, span, _)) => {
                        operations.push(AtomicRaceOperation {
                            op_type: AtomicOpType::Load,
                            transition: t,
                            var_id: var_id.clone(),
                            ordering: format!("{:?}", ordering),
                            span: span.clone(),
                        });
                    }
                    ControlType::Call(CallType::AtomicStore(var_id, ordering, span, _)) => {
                        operations.push(AtomicRaceOperation {
                            op_type: AtomicOpType::Store,
                            transition: t,
                            var_id: var_id.clone(),
                            ordering: format!("{:?}", ordering),
                            span: span.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        // 检查冲突
        for i in 0..operations.len() {
            for j in i + 1..operations.len() {
                if operations[i].var_id == operations[j].var_id {
                    // store-store 冲突
                    if operations[i].op_type == AtomicOpType::Store
                        && operations[j].op_type == AtomicOpType::Store
                    {
                        has_race = true;
                    }
                    // store-load 冲突 (当 load 是 Relaxed 时)
                    else if (operations[i].op_type == AtomicOpType::Store
                        && operations[j].op_type == AtomicOpType::Load
                        && operations[j].ordering == "Relaxed")
                        || (operations[i].op_type == AtomicOpType::Load
                            && operations[j].op_type == AtomicOpType::Store
                            && operations[i].ordering == "Relaxed")
                    {
                        has_race = true;
                    }
                }
            }
        }

        if has_race {
            Some(AtomicRaceInfo {
                state: current_state.clone(),
                operations,
            })
        } else {
            None
        }
    }

    /// Run deadlock detection
    pub fn detect_deadlock(&self) -> String {
        use crate::detect::deadlock::DeadlockDetector;

        let detector = DeadlockDetector::new(self);
        let report = detector.detect();

        format!("{}", report)
    }

    #[allow(dead_code)]
    pub fn dot(&self) -> std::io::Result<()> {
        if self.options.dump_options.dump_state_graph {
            let output_path = self
                .options
                .output
                .as_ref()
                .unwrap()
                .join("state_graph.dot");

            let mut file = std::fs::File::create(&output_path)?;

            // 使用 petgraph 的 Dot 结构直接写入
            write!(file, "{:?}", Dot::new(&self.graph))?;

            log::info!("State graph saved to {}", output_path.display());
        }
        Ok(())
    }

    fn set_current_mark(&self, mark: &HashSet<(NodeIndex, u8)>) {
        // First clear all place tokens to zero
        for node_index in self.initial_net.borrow().node_indices() {
            if let Some(PetriNetNode::P(place)) = self.initial_net.borrow().node_weight(node_index)
            {
                *place.tokens.borrow_mut() = 0u8;
            }
        }

        // Set tokens directly based on the NodeIndex in mark
        for (node_index, token_count) in mark {
            if let Some(PetriNetNode::P(place)) = self.initial_net.borrow().node_weight(*node_index)
            {
                *place.tokens.borrow_mut() = *token_count;
                assert!(
                    *place.tokens.borrow() <= place.capacity,
                    "Token count ({}) exceeds capacity ({}) at node index {}, and token_count is {} ",
                    *place.tokens.borrow(),
                    place.capacity,
                    node_index.index(),
                    token_count
                );
            }
        }
    }

    /// Fire a transition and generate a new network state
    /// 1. Clone current network to create new graph
    /// 2. Set initial tokens based on current marking
    /// 3. Subtract tokens from input places of the transition
    /// 4. Add tokens to output places of the transition (considering capacity limits)
    /// 5. Generate and return new state
    pub fn fire_transition(
        &mut self,
        mark: &HashSet<(NodeIndex, u8)>,
        transition: NodeIndex,
    ) -> Result<HashSet<(NodeIndex, u8)>, String> {
        self.set_current_mark(mark);
        let mut new_state = HashSet::<(NodeIndex, u8)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        // Subtract tokens from input places
        log::debug!("sub token to source node!");
        for edge in self
            .initial_net
            .borrow()
            .edges_directed(transition, Direction::Incoming)
        {
            match self
                .initial_net
                .borrow()
                .node_weight(edge.source())
                .unwrap()
            {
                PetriNetNode::P(place) => {
                    let label = edge.weight().label;
                    if *place.tokens.borrow() < label {
                        return Err(format!(
                            "Place {} has insufficient tokens: required {}, actual {}",
                            place.name,
                            label,
                            *place.tokens.borrow()
                        ));
                    }
                    *place.tokens.borrow_mut() -= label;
                }
                PetriNetNode::T(_) => {
                    return Err("Found input edge connected to transition node".to_string());
                }
            }
        }

        // Add tokens to output places
        log::debug!("add token to target node!");
        for edge in self
            .initial_net
            .borrow()
            .edges_directed(transition, Direction::Outgoing)
        {
            match self
                .initial_net
                .borrow()
                .node_weight(edge.target())
                .unwrap()
            {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.borrow_mut();
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
        for node in self.initial_net.borrow().node_indices() {
            match &self.initial_net.borrow()[node] {
                PetriNetNode::P(place) => {
                    let tokens = *place.tokens.borrow();
                    if tokens > 0 {
                        // 确保token数量不超过容量限制
                        let final_tokens = tokens.min(place.capacity);
                        new_state.insert((node, final_tokens));
                    }
                }
                PetriNetNode::T(_) => {}
            }
        }

        Ok(new_state) // 返回新图和新状态
    }

    /// Get all enabled transitions under current marking
    /// 1. Use `set_current_mark` function to set current marking
    /// 2. Traverse each node in the network to check if it's a transition node
    /// 3. For each transition node, check if all its input places have sufficient tokens
    /// 4. If all input places have sufficient tokens, the transition is enabled
    /// 5. Add all enabled transition node indices to the returned vector
    pub fn get_enabled_transitions(&mut self, mark: &HashSet<(NodeIndex, u8)>) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();

        // Set current marking using inline function
        self.set_current_mark(mark);

        // Logic to check transition enablement
        for node_index in self.initial_net.borrow().node_indices() {
            match self.initial_net.borrow().node_weight(node_index) {
                Some(PetriNetNode::T(_)) => {
                    let mut enabled = true;
                    for edge in self
                        .initial_net
                        .borrow()
                        .edges_directed(node_index, Direction::Incoming)
                    {
                        match self
                            .initial_net
                            .borrow()
                            .node_weight(edge.source())
                            .unwrap()
                        {
                            PetriNetNode::P(place) => {
                                let tokens = place.tokens.borrow();
                                if *tokens < edge.weight().label {
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
}
