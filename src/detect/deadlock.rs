use crate::graph::net_structure::PetriNetNode;
use crate::graph::net_structure::{CallType, ControlType};
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

        let reachability_deadlocks = self.detect_reachability_deadlock();

        let dependency_deadlocks = HashSet::new();

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

                let trace = self.create_deadlock_trace(deadlock_state);
                report.traces.push(trace);
            }
        }

        report.state_space_info = Some(self.collect_state_space_info());

        report.analysis_time = start_time.elapsed();
        report
    }

    fn detect_reachability_deadlock(&self) -> HashSet<Vec<(usize, u8)>> {
        let mut deadlocks = HashSet::new();

        for node_idx in self.state_graph.graph.node_indices() {
            let state = &self.state_graph.graph[node_idx];

            let is_terminal = self.state_graph.graph.edges(node_idx).count() == 0;

            let is_normal_termination = state.mark.iter().any(|(idx, _)| {
                if let Some(PetriNetNode::P(place)) = self
                    .state_graph
                    .initial_net
                    .borrow()
                    .node_weight(node_index(*idx))
                {
                    place.name.contains("main_end")
                } else {
                    false
                }
            });

            if is_terminal && !is_normal_termination {
                deadlocks.insert(state.mark.clone());
            }
        }

        if deadlocks.is_empty() {
            log::info!("no deadlock detected by reachability");

            let cycle_deadlocks = self.detect_cycle_deadlocks();
            deadlocks.extend(cycle_deadlocks);
        }

        deadlocks
    }

    fn detect_cycle_deadlocks(&self) -> HashSet<Vec<(usize, u8)>> {
        let mut deadlocks = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();
        let mut cycle_groups = HashMap::new();

        for start_node in self.state_graph.graph.node_indices() {
            if !visited.contains(&start_node) {
                self.find_deadlock_cycles(
                    start_node,
                    &mut visited,
                    &mut stack,
                    &mut cycle_groups,
                    &Vec::new(),
                );
            }
        }

        for (blocked_transitions, states) in cycle_groups {
            if !blocked_transitions.is_empty() {
                if let Some(state) = states.into_iter().next() {
                    deadlocks.insert(state);
                }
            }
        }

        deadlocks
    }

    fn find_deadlock_cycles(
        &self,
        current: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        stack: &mut HashSet<NodeIndex>,
        cycle_groups: &mut HashMap<Vec<NodeIndex>, HashSet<Vec<(usize, u8)>>>,
        current_path: &Vec<NodeIndex>,
    ) {
        visited.insert(current);
        stack.insert(current);
        let mut path = current_path.clone();
        path.push(current);

        for edge in self.state_graph.graph.edges(current) {
            let next = edge.target();

            if !visited.contains(&next) {
                self.find_deadlock_cycles(next, visited, stack, cycle_groups, &path);
            } else if stack.contains(&next) {
                let cycle_start_idx = path.iter().position(|&x| x == next).unwrap();
                let cycle = &path[cycle_start_idx..];

                if let Some(blocked_trans) = self.get_consistently_blocked_transitions(cycle) {
                    if !blocked_trans.is_empty() {
                        cycle_groups
                            .entry(blocked_trans.into_iter().collect::<Vec<_>>())
                            .or_insert_with(HashSet::new)
                            .extend(cycle.iter().filter_map(|&node| {
                                self.state_graph
                                    .graph
                                    .node_weight(node)
                                    .map(|state| state.mark.clone())
                            }));
                    }
                }
            }
        }

        stack.remove(&current);
    }

    fn get_consistently_blocked_transitions(
        &self,
        cycle: &[NodeIndex],
    ) -> Option<HashSet<NodeIndex>> {
        let lock_transitions = self.collect_lock_transitions();
        let mut consistently_blocked = HashSet::new();
        let all_locks: HashSet<_> = lock_transitions.keys().cloned().collect();

        if let Some(&first_node) = cycle.first() {
            for (lock, transitions) in &lock_transitions {
                let mut is_blocked = true;
                for &trans in transitions {
                    if self.is_transition_enabled(first_node, trans) {
                        is_blocked = false;
                        break;
                    }
                }
                if is_blocked {
                    consistently_blocked.insert(*lock);
                }
            }
        }

        for &node in &cycle[1..] {
            let mut current_blocked = HashSet::new();
            for &lock in &consistently_blocked {
                if let Some(transitions) = lock_transitions.get(&lock) {
                    let mut is_blocked = true;
                    for &trans in transitions {
                        if self.is_transition_enabled(node, trans) {
                            is_blocked = false;
                            break;
                        }
                    }
                    if is_blocked {
                        current_blocked.insert(lock);
                    }
                }
            }
            consistently_blocked = consistently_blocked
                .intersection(&current_blocked)
                .cloned()
                .collect();

            if consistently_blocked.is_empty() {
                return None;
            }
        }

        let is_stable = cycle.iter().all(|&node| {
            self.state_graph
                .graph
                .edges(node)
                .all(|edge| cycle.contains(&edge.target()))
        });

        if all_locks.is_subset(&consistently_blocked) {
            return None;
        }

        if is_stable {
            Some(consistently_blocked)
        } else {
            None
        }
    }

    fn is_deadlock_cycle(&self, cycle: &[NodeIndex]) -> bool {
        let has_terminal_state = cycle.iter().any(|&node| {
            if let Some(state) = self.state_graph.graph.node_weight(node) {
                state.mark.iter().any(|(idx, _)| {
                    if let Some(PetriNetNode::P(place)) = self
                        .state_graph
                        .initial_net
                        .borrow()
                        .node_weight(node_index(*idx))
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

        let lock_transitions = self.collect_lock_transitions();
        let mut consistently_blocked = HashSet::new();

        if let Some(&first_node) = cycle.first() {
            for (lock, transitions) in &lock_transitions {
                let mut is_blocked = true;
                for &trans in transitions {
                    if self.is_transition_enabled(first_node, trans) {
                        is_blocked = false;
                        break;
                    }
                }
                if is_blocked {
                    consistently_blocked.insert(*lock);
                }
            }
        }

        for &node in &cycle[1..] {
            let mut current_blocked = HashSet::new();
            for &lock in &consistently_blocked {
                if let Some(transitions) = lock_transitions.get(&lock) {
                    let mut is_blocked = true;
                    for &trans in transitions {
                        if self.is_transition_enabled(node, trans) {
                            is_blocked = false;
                            break;
                        }
                    }
                    if is_blocked {
                        current_blocked.insert(lock);
                    }
                }
            }
            consistently_blocked = consistently_blocked
                .intersection(&current_blocked)
                .cloned()
                .collect();

            if consistently_blocked.is_empty() {
                return false;
            }
        }

        let is_stable = cycle.iter().all(|&node| {
            self.state_graph
                .graph
                .edges(node)
                .all(|edge| cycle.contains(&edge.target()))
        });

        !consistently_blocked.is_empty() && is_stable
    }

    fn collect_lock_transitions(&self) -> HashMap<NodeIndex, Vec<NodeIndex>> {
        let mut lock_transitions = HashMap::new();

        for node in self.state_graph.initial_net.borrow().node_indices() {
            if let PetriNetNode::T(transition) = &self.state_graph.initial_net.borrow()[node] {
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

    fn is_transition_enabled(&self, state: NodeIndex, transition: NodeIndex) -> bool {
        if let Some(state_node) = self.state_graph.graph.node_weight(state) {
            for edge in self
                .state_graph
                .initial_net
                .borrow()
                .edges_directed(transition, petgraph::Direction::Incoming)
            {
                if let Some(PetriNetNode::P(_)) = self
                    .state_graph
                    .initial_net
                    .borrow()
                    .node_weight(edge.source())
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

    fn format_deadlock_state(&self, mark: &[(usize, u8)]) -> DeadlockState {
        let marking: Vec<(String, u8)> = mark
            .iter()
            .filter_map(|(idx, tokens)| {
                if let Some(PetriNetNode::P(place)) = self
                    .state_graph
                    .initial_net
                    .borrow()
                    .node_weight(node_index(*idx))
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

    fn create_deadlock_trace(&self, deadlock_state: &[(usize, u8)]) -> DeadlockTrace {
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
