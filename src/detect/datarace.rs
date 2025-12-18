use crate::analysis::reachability::StateGraph;
use crate::net::index_vec::Idx;
use crate::net::structure::TransitionType;
use crate::report::{RaceCondition, RaceOperation, RaceReport};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::time::Instant;

pub struct DataRaceDetector<'a> {
    state_graph: &'a StateGraph,
}

impl<'a> DataRaceDetector<'a> {
    pub fn new(state_graph: &'a StateGraph) -> Self {
        Self { state_graph }
    }

    pub fn detect(&self) -> RaceReport {
        let start_time = Instant::now();
        let mut report = RaceReport::new("State Graph Data Race Detector".to_string());
        let mut race_infos = Vec::new();

        for state in self.state_graph.graph.node_indices() {
            let transitions = self.collect_state_accesses(state);
            if transitions.len() < 2 {
                continue;
            }

            let state_snapshot = self.state_snapshot(state);
            self.check_race_in_state(&transitions, state_snapshot, &mut race_infos);
        }

        let race_conditions = self.merge_race_conditions(race_infos);

        if !race_conditions.is_empty() {
            report.has_race = true;
            report.race_count = race_conditions.len();
            report.race_conditions = race_conditions;
        }

        report.analysis_time = start_time.elapsed();
        report
    }

    fn check_race_in_state(
        &self,
        transitions: &[StateAccess],
        state_marks: Vec<(usize, u8)>,
        race_infos: &mut Vec<RaceCondition>,
    ) {
        for (i, access_a) in transitions.iter().enumerate() {
            for access_b in transitions.iter().skip(i + 1) {
                if access_a.location_id == access_b.location_id
                    && (access_a.is_write || access_b.is_write)
                {
                    let operations = vec![
                        RaceOperation {
                            operation_type: access_a.op_type.to_string(),
                            variable: access_a.data_type.clone(),
                            location: access_a.span.clone(),
                            basic_block: Some(access_a.basic_block),
                        },
                        RaceOperation {
                            operation_type: access_b.op_type.to_string(),
                            variable: access_b.data_type.clone(),
                            location: access_b.span.clone(),
                            basic_block: Some(access_b.basic_block),
                        },
                    ];

                    race_infos.push(RaceCondition {
                        operations,
                        variable_info: format!("变量 {} 上的潜在数据竞争", access_a.location_id),
                        state: state_marks.clone(),
                    });
                }
            }
        }
    }

    fn merge_race_conditions(&self, conditions: Vec<RaceCondition>) -> Vec<RaceCondition> {
        let mut merged = HashMap::new();

        for condition in conditions {
            let key = (
                condition.variable_info.clone(),
                condition.operations[0].basic_block.clone(),
                condition.operations[0].location.clone(),
            );

            merged
                .entry(key)
                .and_modify(|existing: &mut RaceCondition| {
                    for op in condition.clone().operations {
                        if !existing
                            .operations
                            .iter()
                            .any(|existing_op| existing_op.operation_type == op.operation_type)
                        {
                            existing.operations.push(op);
                        }
                    }
                })
                .or_insert(condition);
        }

        merged.into_values().collect()
    }

    fn collect_state_accesses(&self, state: NodeIndex) -> Vec<StateAccess> {
        let mut accesses = Vec::new();

        for edge in self.state_graph.graph.edges(state) {
            match &edge.weight().transition.transition_type {
                TransitionType::UnsafeRead(alias_id, span, basic_block, place_ty) => {
                    accesses.push(StateAccess {
                        location_id: *alias_id,
                        span: span.clone(),
                        basic_block: *basic_block,
                        op_type: "read",
                        data_type: place_ty.clone(),
                        is_write: false,
                    });
                }
                TransitionType::UnsafeWrite(alias_id, span, basic_block, place_ty) => {
                    accesses.push(StateAccess {
                        location_id: *alias_id,
                        span: span.clone(),
                        basic_block: *basic_block,
                        op_type: "write",
                        data_type: place_ty.clone(),
                        is_write: true,
                    });
                }
                _ => {}
            }
        }

        accesses
    }

    fn state_snapshot(&self, state: NodeIndex) -> Vec<(usize, u8)> {
        let node = self.state_graph.node(state);
        node.marking
            .iter()
            .filter_map(|(place_id, tokens)| {
                if *tokens == 0 {
                    return None;
                }
                Some((place_id.index(), (*tokens).min(u8::MAX as u64) as u8))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct StateAccess {
    location_id: usize,
    span: String,
    basic_block: usize,
    op_type: &'static str,
    data_type: String,
    is_write: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::reachability::StateGraph;
    use crate::net::Net;
    use crate::net::structure::{Place, PlaceType, Transition, TransitionType};

    fn build_data_race_net() -> Net {
        let mut net = Net::empty();
        let shared = net.add_place(Place::new(
            "shared",
            1,
            1,
            PlaceType::BasicBlock,
            "shared.rs:1:1".into(),
        ));

        let read = net.add_transition(Transition::new_with_transition_type(
            "unsafe_read",
            TransitionType::UnsafeRead(0, "shared.rs:10:5".into(), 0, "i32".into()),
        ));
        let write = net.add_transition(Transition::new_with_transition_type(
            "unsafe_write",
            TransitionType::UnsafeWrite(0, "shared.rs:20:5".into(), 0, "i32".into()),
        ));

        net.set_input_weight(shared, read, 1);
        net.set_output_weight(shared, read, 1);

        net.set_input_weight(shared, write, 1);
        net.set_output_weight(shared, write, 1);

        net
    }

    #[test]
    fn detect_simple_data_race() {
        let net = build_data_race_net();
        let state_graph = StateGraph::from_net(&net);
        let detector = DataRaceDetector::new(&state_graph);
        let report = detector.detect();

        assert!(report.has_race, "Expected data race to be detected");
        assert_eq!(report.race_count, 1);
        let race = &report.race_conditions[0];
        assert_eq!(race.operations.len(), 2);
        assert!(
            race.operations
                .iter()
                .any(|op| op.operation_type == "write")
        );
    }
}
