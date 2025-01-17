use petgraph::{graph::NodeIndex, visit::EdgeRef, Direction, Graph};
use rustc_hir::def_id::DefId;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::options::Options;

use super::pn::{PetriNetEdge, PetriNetNode};

#[derive(Debug)]
struct UnfoldingEvent {
    conditions: HashSet<NodeIndex>,  // 库所实例
    transitions: HashSet<NodeIndex>, // 变迁实例
    marks: HashSet<(NodeIndex, u8)>, // 当前标记
}

#[derive(Debug, Clone)]
pub struct UnfoldingNet {
    pub initial_net: Graph<PetriNetNode, PetriNetEdge>,
    pub initial_mark: HashSet<(NodeIndex, u8)>,

    function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    options: Options,
}

impl UnfoldingNet {
    pub fn new(
        graph: Graph<PetriNetNode, PetriNetEdge>,
        mark: HashSet<(NodeIndex, u8)>,
        function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
        options: Options,
    ) -> Self {
        UnfoldingNet {
            initial_net: graph,
            initial_mark: mark,
            function_counter,

            options,
        }
    }

    pub fn check_local_deadlock(&self) -> Option<Vec<NodeIndex>> {
        let mut unfolding = UnfoldingEvent {
            conditions: HashSet::new(),
            transitions: HashSet::new(),
            marks: self.initial_mark.clone(),
        };

        self.unfold_net(&mut unfolding)
    }

    fn unflod_enabled_transitions(&self, mark: &HashSet<(NodeIndex, u8)>) -> Vec<NodeIndex> {
        self.set_petrinet_mark(mark);
        let mut sched_transiton = Vec::<NodeIndex>::new();
        // 检查变迁使能的逻辑
        for node_index in self.initial_net.node_indices() {
            match self.initial_net.node_weight(node_index) {
                Some(PetriNetNode::T(_)) => {
                    let mut enabled = true;
                    for edge in self
                        .initial_net
                        .edges_directed(node_index, Direction::Incoming)
                    {
                        match self.initial_net.node_weight(edge.source()).unwrap() {
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

    fn unfold_fire_transition(
        &self,
        mark: &HashSet<(NodeIndex, u8)>,
        transition: NodeIndex,
    ) -> HashSet<(NodeIndex, u8)> {
        self.set_petrinet_mark(mark);
        let mut new_state = HashSet::<(NodeIndex, u8)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        // 从输入库所中减去token
        log::debug!("sub token to source node!");
        for edge in self
            .initial_net
            .edges_directed(transition, Direction::Incoming)
        {
            match self.initial_net.node_weight(edge.source()).unwrap() {
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
        for edge in self
            .initial_net
            .edges_directed(transition, Direction::Outgoing)
        {
            let place_node = self.initial_net.node_weight(edge.target()).unwrap();
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
        for node in self.initial_net.node_indices() {
            match &self.initial_net[node] {
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

        new_state
    }

    fn set_petrinet_mark(&self, mark: &HashSet<(NodeIndex, u8)>) {
        // 首先将所有库所的 token 清零
        for node_index in self.initial_net.node_indices() {
            if let Some(PetriNetNode::P(place)) = self.initial_net.node_weight(node_index) {
                *place.tokens.write().unwrap() = 0;
            }
        }

        // 直接根据 mark 中的 NodeIndex 设置对应的 token
        for (node_index, token_count) in mark {
            if let Some(PetriNetNode::P(place)) = self.initial_net.node_weight(*node_index) {
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

    fn unfold_net(&self, unfolding: &mut UnfoldingEvent) -> Option<Vec<NodeIndex>> {
        let mut work_queue = VecDeque::new();
        work_queue.push_back(unfolding.marks.clone());

        let mut visited = HashSet::new();
        visited.insert(self.marks_to_key(&unfolding.marks));

        while let Some(current_marks) = work_queue.pop_front() {
            // 获取当前可发生的变迁
            let enabled = self.unflod_enabled_transitions(&current_marks);

            // 检查死锁
            if enabled.is_empty() && !self.is_final_marking(&current_marks) {
                return Some(self.reconstruct_path(&current_marks));
            }

            // 尝试发生每个变迁
            for t in enabled {
                if !self.has_conflict(t, &unfolding.transitions) {
                    let new_marks = self.unfold_fire_transition(&current_marks, t);
                    let marks_key = self.marks_to_key(&new_marks);

                    if !visited.contains(&marks_key) {
                        visited.insert(marks_key);
                        work_queue.push_back(new_marks.clone());
                        unfolding.transitions.insert(t);

                        // 更新条件集
                        for (place, _) in &new_marks {
                            unfolding.conditions.insert(*place);
                        }
                    }
                }
            }
        }

        None // 没有发现死锁
    }

    fn is_final_marking(&self, marks: &HashSet<(NodeIndex, u8)>) -> bool {
        // 检查是否是最终标记（所有终止库所都有token）
        for (node, _) in marks.iter() {
            if let PetriNetNode::P(ref place) = self.initial_net[*node] {
                if place.name.starts_with("main_end") && self.initial_net.edges(*node).count() == 0
                {
                    return true;
                }
            }
        }
        false
    }

    fn has_conflict(&self, t: NodeIndex, existing: &HashSet<NodeIndex>) -> bool {
        for exist_t in existing {
            if self.are_transitions_conflicting(t, *exist_t) {
                return true;
            }
        }
        false
    }

    fn are_transitions_conflicting(&self, t1: NodeIndex, t2: NodeIndex) -> bool {
        let t1_inputs: HashSet<_> = self
            .initial_net
            .edges_directed(t1, petgraph::Direction::Incoming)
            .map(|e| e.source())
            .collect();

        let t2_inputs: HashSet<_> = self
            .initial_net
            .edges_directed(t2, petgraph::Direction::Incoming)
            .map(|e| e.source())
            .collect();

        !t1_inputs.is_disjoint(&t2_inputs)
    }

    fn marks_to_key(&self, marks: &HashSet<(NodeIndex, u8)>) -> String {
        // 将标记转换为唯一的字符串标识
        let mut items: Vec<_> = marks.iter().collect();
        items.sort_by_key(|&(idx, _)| idx.index());
        items
            .iter()
            .map(|(idx, count)| format!("{}:{}", idx.index(), count))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn reconstruct_path(&self, deadlock_marks: &HashSet<(NodeIndex, u8)>) -> Vec<NodeIndex> {
        // 构建导致死锁的路径
        let mut path = Vec::new();
        let mut current = deadlock_marks.clone();

        // TODO: 实现路径重建逻辑
        // 这里需要根据具体需求实现从死锁标记回溯到初始标记的路径

        path
    }
}
