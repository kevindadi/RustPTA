use crate::graph::pn::PetriNetNode;
use crate::graph::state_graph::StateGraph;
use crate::report::{DeadlockReport, DeadlockState, DeadlockTrace};
use petgraph::graph::{node_index, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub struct DeadlockDetector<'a> {
    state_graph: &'a StateGraph,
}

impl<'a> DeadlockDetector<'a> {
    pub fn new(state_graph: &'a StateGraph) -> Self {
        Self { state_graph }
    }

    pub fn detect(&self) -> DeadlockReport {
        let start_time = Instant::now();
        let mut report = DeadlockReport::new("Petri Net Deadlock Detector".to_string());

        // 运行基于状态可达性的死锁检测
        let reachability_deadlocks = self.detect_reachability_deadlock();

        // 运行基于锁依赖的死锁检测
        // let dependency_deadlocks = self.detect_lock_dependency_deadlock();
        let dependency_deadlocks = HashSet::new();
        // 合并结果
        let all_deadlocks: HashSet<_> = reachability_deadlocks
            .into_iter()
            .chain(dependency_deadlocks.into_iter())
            .collect();

        if !all_deadlocks.is_empty() {
            report.has_deadlock = true;
            report.deadlock_count = all_deadlocks.len();

            for (_, deadlock_state) in all_deadlocks.iter().enumerate() {
                let state_info = self.format_deadlock_state(deadlock_state);
                report.deadlock_states.push(state_info);

                // 为每个死锁状态创建一个追踪路径
                let trace = self.create_deadlock_trace(deadlock_state);
                report.traces.push(trace);
            }
        }

        // 添加状态空间信息
        report.state_space_info = Some(self.collect_state_space_info());

        report.analysis_time = start_time.elapsed();
        report
    }

    /// 基于状态可达性的死锁检测
    fn detect_reachability_deadlock(&self) -> HashSet<Vec<(usize, usize)>> {
        let mut deadlocks = HashSet::new();

        // 1. 检测终止状态死锁
        for node_idx in self.state_graph.graph.node_indices() {
            let state = &self.state_graph.graph[node_idx];

            // 检查是否是终止状态
            let is_terminal = self.state_graph.graph.edges(node_idx).count() == 0;

            // 检查是否是正常终止状态
            let is_normal_termination = state.mark.iter().any(|(idx, _)| {
                if let Some(PetriNetNode::P(place)) =
                    self.state_graph.initial_net.node_weight(node_index(*idx))
                {
                    place.name.contains("main_end")
                } else {
                    false
                }
            });

            // 如果是终止状态但不是正常终止，则是死锁
            if is_terminal && !is_normal_termination {
                deadlocks.insert(state.mark.clone());
            }
        }

        // 2. 检测环路死锁
        let cycle_deadlocks = self.detect_cycle_deadlocks();
        deadlocks.extend(cycle_deadlocks);

        deadlocks
    }

    /// 检测环路死锁
    fn detect_cycle_deadlocks(&self) -> HashSet<Vec<(usize, usize)>> {
        let mut deadlocks = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();

        // 从每个节点开始搜索环路
        for start_node in self.state_graph.graph.node_indices() {
            if !visited.contains(&start_node) {
                self.find_deadlock_cycles(
                    start_node,
                    &mut visited,
                    &mut stack,
                    &mut deadlocks,
                    &Vec::new(),
                );
            }
        }

        deadlocks
    }

    fn find_deadlock_cycles(
        &self,
        current: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        stack: &mut HashSet<NodeIndex>,
        deadlocks: &mut HashSet<Vec<(usize, usize)>>,
        current_path: &Vec<NodeIndex>,
    ) {
        visited.insert(current);
        stack.insert(current);
        let mut path = current_path.clone();
        path.push(current);

        // 检查当前节点的所有后继
        for edge in self.state_graph.graph.edges(current) {
            let next = edge.target();

            if !visited.contains(&next) {
                // 继续DFS
                self.find_deadlock_cycles(next, visited, stack, deadlocks, &path);
            } else if stack.contains(&next) {
                // 找到环路，检查是否是死锁环路
                let cycle_start_idx = path.iter().position(|&x| x == next).unwrap();
                let cycle = &path[cycle_start_idx..];

                if self.is_deadlock_cycle(cycle) {
                    // 将环路中的所有状态添加到死锁集合
                    for &node in cycle {
                        if let Some(state) = self.state_graph.graph.node_weight(node) {
                            deadlocks.insert(state.mark.clone());
                        }
                    }
                }
            }
        }

        stack.remove(&current);
    }

    /// 判断一个环路是否是死锁环路
    fn is_deadlock_cycle(&self, cycle: &[NodeIndex]) -> bool {
        // 1. 检查环路中是否包含终止状态
        let has_terminal_state = cycle.iter().any(|&node| {
            if let Some(state) = self.state_graph.graph.node_weight(node) {
                state.mark.iter().any(|(idx, _)| {
                    if let Some(PetriNetNode::P(place)) =
                        self.state_graph.initial_net.node_weight(node_index(*idx))
                    {
                        place.name.contains("main_end")
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        });

        if has_terminal_state {
            return false;
        }

        // 2. 检查是否存在被永久阻塞的锁操作
        let lock_transitions = self.collect_lock_transitions();
        let mut permanently_blocked = HashSet::new();

        for &node in cycle {
            for (lock, transitions) in &lock_transitions {
                let mut is_blocked = true;
                for &trans in transitions {
                    if self.is_transition_enabled(node, trans) {
                        is_blocked = false;
                        break;
                    }
                }
                if is_blocked {
                    permanently_blocked.insert(lock);
                }
            }
        }

        // 3. 检查环路的稳定性
        let is_stable = cycle.iter().all(|&node| {
            self.state_graph
                .graph
                .edges(node)
                .all(|edge| cycle.contains(&edge.target()))
        });

        !permanently_blocked.is_empty() && is_stable
    }

    /// 收集所有锁相关的变迁
    fn collect_lock_transitions(&self) -> HashMap<NodeIndex, Vec<NodeIndex>> {
        let mut lock_transitions = HashMap::new();

        for node in self.state_graph.initial_net.node_indices() {
            if let PetriNetNode::T(transition) = &self.state_graph.initial_net[node] {
                use crate::graph::pn::{CallType, ControlType};
                match &transition.transition_type {
                    ControlType::Call(CallType::Lock(lock_place))
                    | ControlType::Call(CallType::RwLockWrite(lock_place))
                    | ControlType::Call(CallType::RwLockRead(lock_place)) => {
                        lock_transitions
                            .entry(*lock_place)
                            .or_insert_with(Vec::new)
                            .push(node);
                    }
                    _ => {}
                }
            }
        }

        lock_transitions
    }

    /// 检查在给定状态下变迁是否可以发生
    fn is_transition_enabled(&self, state: NodeIndex, transition: NodeIndex) -> bool {
        if let Some(state_node) = self.state_graph.graph.node_weight(state) {
            for edge in self
                .state_graph
                .initial_net
                .edges_directed(transition, petgraph::Direction::Incoming)
            {
                if let Some(PetriNetNode::P(_)) =
                    self.state_graph.initial_net.node_weight(edge.source())
                {
                    let required_tokens = edge.weight().label;
                    let available_tokens = state_node
                        .mark
                        .iter()
                        .find(|(idx, _)| *idx == edge.source().index())
                        .map(|(_, tokens)| *tokens)
                        .unwrap_or(0);

                    if available_tokens < required_tokens {
                        return false;
                    }
                }
            }
            return true;
        }
        false
    }

    fn format_deadlock_state(&self, mark: &[(usize, usize)]) -> DeadlockState {
        let marking: Vec<(String, usize)> = mark
            .iter()
            .filter_map(|(idx, tokens)| {
                if let Some(PetriNetNode::P(place)) =
                    self.state_graph.initial_net.node_weight(node_index(*idx))
                {
                    Some((format!("{} ({})", place.name, place.span), *tokens))
                } else {
                    None
                }
            })
            .collect();

        DeadlockState {
            state_id: format!("s{}", mark.iter().map(|(i, _)| i).sum::<usize>()),
            marking,
            description: "Deadlock state with blocked resources".to_string(),
        }
    }

    /// 创建到达死锁状态的路径
    fn create_deadlock_trace(&self, deadlock_state: &[(usize, usize)]) -> DeadlockTrace {
        // TODO: 实现路径重建逻辑
        DeadlockTrace {
            steps: vec!["Path reconstruction not implemented yet".to_string()],
            final_state: Some(self.format_deadlock_state(deadlock_state)),
        }
    }

    fn collect_state_space_info(&self) -> crate::report::StateSpaceInfo {
        crate::report::StateSpaceInfo {
            total_states: self.state_graph.graph.node_count(),
            total_transitions: self.state_graph.graph.edge_count(),
            reachable_states: self.state_graph.graph.node_count(),
        }
    }
}
