use crate::analysis::reachability::StateGraph;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::index_vec::Idx;
use crate::net::structure::TransitionType;
use crate::report::{DeadlockReport, DeadlockState, DeadlockTrace};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
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

        let dependency_deadlocks = FxHashSet::default();

        let all_deadlocks: FxHashSet<_> = reachability_deadlocks
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

    fn detect_reachability_deadlock(&self) -> FxHashSet<NodeIndex> {
        let mut deadlocks = FxHashSet::default();

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

    fn detect_cycle_deadlocks(&self) -> FxHashSet<NodeIndex> {
        let mut deadlocks = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut stack = FxHashSet::default();
        let mut cycle_groups: FxHashMap<Vec<usize>, FxHashSet<NodeIndex>> = FxHashMap::default();

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
        visited: &mut FxHashSet<NodeIndex>,
        stack: &mut FxHashSet<NodeIndex>,
        cycle_groups: &mut FxHashMap<Vec<usize>, FxHashSet<NodeIndex>>,
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
                            .or_default()
                            .extend(cycle);
                    }
                }
            }
        }

        stack.remove(&current);
    }

    fn get_consistently_blocked_transitions(&self, cycle: &[NodeIndex]) -> Option<FxHashSet<usize>> {
        let lock_transitions = self.collect_lock_transitions();
        let mut consistently_blocked = FxHashSet::default();
        let all_locks: FxHashSet<_> = lock_transitions.keys().cloned().collect();

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
            let mut current_blocked = FxHashSet::default();
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

    fn collect_lock_transitions(&self) -> FxHashMap<usize, Vec<TransitionId>> {
        let mut lock_transitions: FxHashMap<usize, Vec<TransitionId>> = FxHashMap::default();

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

    fn find_blocked_resource_transitions(
        &self,
        state: NodeIndex,
    ) -> Vec<crate::report::BlockedTransition> {
        use crate::net::structure::PlaceType;
        use crate::report::{BlockedTransition, ResourceStatus};

        let state_node = self.state_graph.node(state);
        let net = match &self.state_graph.net {
            Some(net) => net,
            None => return Vec::new(),
        };
        let token_count = |place: PlaceId| state_node.marking.0.get(place).copied().unwrap_or(0);
        let place_names: FxHashMap<PlaceId, (String, String)> = net
            .places
            .iter_enumerated()
            .map(|(place_id, place)| (place_id, (place.name.clone(), place.span.clone())))
            .collect();
        let mut blocked = Vec::new();

        for (transition_id, transition) in net.transitions.iter_enumerated() {
            let is_resource_operation = matches!(
                &transition.transition_type,
                TransitionType::Lock(_)
                    | TransitionType::RwLockRead(_)
                    | TransitionType::RwLockWrite(_)
                    | TransitionType::UnsafeRead(_, _, _, _)
                    | TransitionType::UnsafeWrite(_, _, _, _)
                    | TransitionType::AtomicLoad(_, _, _, _)
                    | TransitionType::AtomicStore(_, _, _, _)
                    | TransitionType::AtomicCmpXchg(_, _, _, _, _)
            );
            if !is_resource_operation || self.is_transition_enabled(state, transition_id) {
                continue;
            }

            let pre_places: Vec<(PlaceId, u64, PlaceType)> = net
                .places
                .iter_enumerated()
                .filter_map(|(place_id, place)| {
                    let weight = *net.pre.get(place_id, transition_id);
                    (weight > 0).then(|| (place_id, weight, place.place_type.clone()))
                })
                .collect();
            if pre_places.is_empty() {
                continue;
            }

            let control_places: Vec<(PlaceId, u64)> = pre_places
                .iter()
                .filter_map(|(place_id, weight, place_type)| {
                    matches!(place_type, PlaceType::BasicBlock | PlaceType::FunctionStart)
                        .then_some((*place_id, *weight))
                })
                .collect();
            let resource_places: Vec<(PlaceId, u64)> = pre_places
                .iter()
                .filter_map(|(place_id, weight, place_type)| {
                    matches!(place_type, PlaceType::Resources).then_some((*place_id, *weight))
                })
                .collect();

            if !control_places
                .iter()
                .any(|(place_id, _)| token_count(*place_id) > 0)
            {
                continue;
            }

            let resource_status: Vec<ResourceStatus> = resource_places
                .iter()
                .filter_map(|(place_id, weight)| {
                    let has = token_count(*place_id);
                    if has >= *weight {
                        return None;
                    }
                    let (resource_name, _) = place_names
                        .get(place_id)
                        .cloned()
                        .unwrap_or_else(|| (format!("place#{}", place_id.index()), String::new()));
                    Some(ResourceStatus {
                        resource_name,
                        has,
                        needs: *weight,
                    })
                })
                .collect();
            if resource_status.is_empty() {
                continue;
            }

            let location = control_places
                .iter()
                .find(|(place_id, _)| token_count(*place_id) > 0)
                .and_then(|(place_id, _)| place_names.get(place_id).map(|(_, span)| span.clone()))
                .unwrap_or_default();
            let operation = match &transition.transition_type {
                TransitionType::Lock(_) => "Lock acquisition",
                TransitionType::RwLockRead(_) => "RwLock read lock",
                TransitionType::RwLockWrite(_) => "RwLock write lock",
                TransitionType::UnsafeRead(_, _, _, _) => "Unsafe read",
                TransitionType::UnsafeWrite(_, _, _, _) => "Unsafe write",
                TransitionType::AtomicLoad(_, _, _, _) => "Atomic load",
                TransitionType::AtomicStore(_, _, _, _) => "Atomic store",
                TransitionType::AtomicCmpXchg(_, _, _, _, _) => "Atomic compare-exchange",
                _ => "Resource operation",
            }
            .to_string();
            let needed_resources = resource_status
                .iter()
                .map(|status| status.resource_name.clone())
                .collect();

            blocked.push(BlockedTransition {
                id: format!("t{}", transition_id.index()),
                name: transition.name.clone(),
                location,
                operation,
                needed_resources,
                resource_status,
            });
        }

        blocked
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
            blocked_transitions: self.find_blocked_resource_transitions(node),
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
    use crate::net::Net;
    use crate::net::structure::{Place, PlaceType, Transition, TransitionType};

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

    #[test]
    fn reports_blocked_resource_transition_even_when_it_has_no_state_graph_edge() {
        let mut net = Net::empty();
        let control = net.add_place(Place::new(
            "main_0_wait",
            1,
            1,
            PlaceType::BasicBlock,
            "src/main.rs:10:5".into(),
        ));
        let resource = net.add_place(Place::new(
            "Mutex_0",
            0,
            1,
            PlaceType::Resources,
            String::new(),
        ));
        let after_lock = net.add_place(Place::new(
            "main_1",
            0,
            1,
            PlaceType::BasicBlock,
            "src/main.rs:11:5".into(),
        ));
        let lock = net.add_transition(Transition::new_with_transition_type(
            "main_0_lock",
            TransitionType::Lock(0),
        ));

        net.add_input_arc(control, lock, 1);
        net.add_input_arc(resource, lock, 1);
        net.add_output_arc(after_lock, lock, 1);

        let state_graph = StateGraph::from_net(&net);
        let detector = DeadlockDetector::new(&state_graph);
        let report = detector.detect();
        let blocked = &report.deadlock_states[0].blocked_transitions;

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].name, "main_0_lock");
        assert_eq!(blocked[0].needed_resources, vec!["Mutex_0"]);
        assert_eq!(blocked[0].resource_status[0].has, 0);
        assert_eq!(blocked[0].resource_status[0].needs, 1);
    }
}
