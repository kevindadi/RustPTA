use crate::analysis::reachability::StateGraph;
use crate::net::ids::TransitionId;
use crate::net::index_vec::Idx;
use crate::net::structure::TransitionType;
use crate::report::{DeadlockReport, DeadlockState, DeadlockTrace};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rustc_hash::FxHashMap;
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

            for deadlock_state in &all_deadlocks {
                let state_info = self.format_deadlock_state(*deadlock_state);
                report.deadlock_states.push(state_info);

                let trace = self.create_deadlock_trace(*deadlock_state);
                report.traces.push(trace);
            }
        }

        report.state_space_info = Some(self.collect_state_space_info());

        report.analysis_time = start_time.elapsed();
        report
    }

    fn detect_reachability_deadlock(&self) -> HashSet<NodeIndex> {
        let mut deadlocks = HashSet::new();

        for node_idx in self.state_graph.graph.node_indices() {
            let state = self.state_graph.node(node_idx);
            let is_terminal = self.state_graph.graph.edges(node_idx).count() == 0;
            let is_normal_termination = state
                .places
                .iter()
                .any(|place| place.tokens > 0 && place.name.contains("main_end"));

            if is_terminal && !is_normal_termination {
                deadlocks.insert(node_idx);
            }
        }

        if deadlocks.is_empty() {
            log::info!("no deadlock detected by reachability");

            let cycle_deadlocks = self.detect_cycle_deadlocks();
            deadlocks.extend(cycle_deadlocks);
        }

        deadlocks
    }

    fn detect_cycle_deadlocks(&self) -> HashSet<NodeIndex> {
        let mut deadlocks = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();
        let mut cycle_groups: FxHashMap<Vec<usize>, HashSet<NodeIndex>> = FxHashMap::default();

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

        for (_blocked_transitions, states) in cycle_groups {
            if let Some(state) = states.into_iter().next() {
                deadlocks.insert(state);
            }
        }

        deadlocks
    }

    fn find_deadlock_cycles(
        &self,
        current: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        stack: &mut HashSet<NodeIndex>,
        cycle_groups: &mut FxHashMap<Vec<usize>, HashSet<NodeIndex>>,
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
                        let mut key: Vec<_> = blocked_trans.into_iter().collect();
                        key.sort_unstable();
                        cycle_groups
                            .entry(key)
                            .or_insert_with(HashSet::new)
                            .extend(cycle);
                    }
                }
            }
        }

        stack.remove(&current);
    }

    fn get_consistently_blocked_transitions(&self, cycle: &[NodeIndex]) -> Option<HashSet<usize>> {
        let lock_transitions = self.collect_lock_transitions();
        let mut consistently_blocked = HashSet::new();
        let all_locks: HashSet<_> = lock_transitions.keys().cloned().collect();

        if let Some(&first_node) = cycle.first() {
            for (lock, transitions) in &lock_transitions {
                let blocked = transitions
                    .iter()
                    .all(|transition| !self.is_transition_enabled(first_node, *transition));
                if blocked {
                    consistently_blocked.insert(*lock);
                }
            }
        }

        for &node in &cycle[1..] {
            let mut current_blocked = HashSet::new();
            for &lock in &consistently_blocked {
                if let Some(transitions) = lock_transitions.get(&lock) {
                    let blocked = transitions
                        .iter()
                        .all(|transition| !self.is_transition_enabled(node, *transition));
                    if blocked {
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

    fn collect_lock_transitions(&self) -> HashMap<usize, Vec<TransitionId>> {
        let mut lock_transitions: HashMap<usize, Vec<TransitionId>> = HashMap::new();

        for edge in self.state_graph.graph.edge_weights() {
            match &edge.transition.transition_type {
                TransitionType::Lock(lock_id)
                | TransitionType::RwLockWrite(lock_id)
                | TransitionType::RwLockRead(lock_id) => {
                    lock_transitions
                        .entry(*lock_id)
                        .or_default()
                        .push(edge.transition.id);
                }
                _ => {}
            }
        }

        for transitions in lock_transitions.values_mut() {
            transitions.sort_unstable_by_key(|id| id.index());
            transitions.dedup_by_key(|id| id.index());
        }

        lock_transitions
    }

    fn is_transition_enabled(&self, state: NodeIndex, transition_id: TransitionId) -> bool {
        let state_node = self.state_graph.node(state);
        state_node
            .enabled
            .iter()
            .any(|summary| summary.id == transition_id)
    }

    fn format_deadlock_state(&self, node: NodeIndex) -> DeadlockState {
        let state = self.state_graph.node(node);
        let marking: Vec<(String, u8)> = state
            .marking
            .iter()
            .filter_map(|(place_id, tokens)| {
                if *tokens == 0 {
                    return None;
                }
                let description = state
                    .places
                    .iter()
                    .find(|p| p.place == place_id)
                    .map(|p| format!("{} ({})", p.name, p.span))
                    .unwrap_or_else(|| format!("place#{}", place_id.index()));
                Some((description, (*tokens).min(u8::MAX as u64) as u8))
            })
            .collect();

        DeadlockState {
            state_id: format!("s{}", state.index),
            marking,
            description: "Deadlock state with blocked resources".to_string(),
        }
    }

    fn create_deadlock_trace(&self, node: NodeIndex) -> DeadlockTrace {
        DeadlockTrace {
            steps: vec!["Path reconstruction not implemented yet".to_string()],
            final_state: Some(self.format_deadlock_state(node)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::reachability::StateGraph;
    use crate::net::structure::{Place, PlaceType, Transition};
    use crate::net::Net;

    fn build_deadlock_net() -> Net {
        let mut net = Net::empty();
        let start = net.add_place(Place::new(
            "start",
            1,
            1,
            PlaceType::BasicBlock,
            "deadlock.rs:1:1".into(),
        ));
        let progress = net.add_place(Place::new(
            "progress",
            0,
            1,
            PlaceType::BasicBlock,
            "deadlock.rs:5:1".into(),
        ));
        let sink = net.add_place(Place::new(
            "blocked",
            0,
            1,
            PlaceType::BasicBlock,
            "deadlock.rs:9:1".into(),
        ));

        let loop_transition = net.add_transition(Transition::new("loop"));
        let block_transition = net.add_transition(Transition::new("block"));

        net.set_input_weight(start, loop_transition, 1);
        net.set_output_weight(progress, loop_transition, 1);

        net.set_input_weight(progress, loop_transition, 1);
        net.set_output_weight(progress, loop_transition, 1);

        net.set_input_weight(start, block_transition, 1);
        net.set_output_weight(sink, block_transition, 1);

        net
    }

    #[test]
    fn detect_simple_deadlock() {
        let net = build_deadlock_net();
        let state_graph = StateGraph::from_net(&net);
        let detector = DeadlockDetector::new(&state_graph);
        let report = detector.detect();

        assert!(report.has_deadlock, "Expected deadlock to be detected");
        assert!(report.deadlock_count >= 1);
        assert!(!report.deadlock_states.is_empty());
    }
}
