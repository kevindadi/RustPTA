//! Data race detection module for concurrent Rust programs.
//!
//! This module implements data race detection algorithms that identify potentially
//! unsafe concurrent memory accesses in Rust programs. It analyzes unsafe memory
//! operations and their synchronization patterns to detect race conditions.
//!
//! ## Detection Strategy
//!
//! The detector operates by:
//! 1. **State Space Analysis**: Examines all reachable states in the Petri net model
//! 2. **Unsafe Operation Tracking**: Identifies unsafe read/write operations on shared data
//! 3. **Concurrency Analysis**: Detects when multiple threads can access the same memory location
//! 4. **Race Condition Validation**: Determines if concurrent accesses constitute actual races
//!
//! ## Race Condition Criteria
//!
//! A data race is detected when:
//! - Two or more threads access the same memory location concurrently
//! - At least one access is a write operation
//! - The accesses are not properly synchronized
//! - The operations occur in the same program state
//!
//! ## Integration Features
//! - Works with alias analysis to track memory relationships
//! - Supports various unsafe operation patterns
//! - Provides detailed reports with source code locations
//! - Merges similar race conditions for cleaner output

use crate::graph::net_structure::{ControlType, PetriNetNode};
use crate::graph::state_graph::StateGraph;
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
        let mut report = RaceReport::new("Data Race Detector".to_string());
        let mut race_infos = Vec::new();

        // Traverse all state nodes
        for state in self.state_graph.graph.node_indices() {
            let mut state_transitions = Vec::new();

            // Collect all unsafe operations in the current state
            for edge in self.state_graph.graph.edges(state) {
                let transition = edge.weight().transition;
                if let Some(node) = self
                    .state_graph
                    .initial_net
                    .borrow()
                    .node_weight(transition)
                {
                    match node {
                        PetriNetNode::T(t) => match &t.transition_type {
                            ControlType::UnsafeRead(alias_id, span, basic_block, place_ty) => {
                                state_transitions.push((
                                    alias_id.clone(),
                                    span.clone(),
                                    *basic_block,
                                    "read",
                                    place_ty.clone(),
                                ));
                            }
                            ControlType::UnsafeWrite(alias_id, span, basic_block, place_ty) => {
                                state_transitions.push((
                                    alias_id.clone(),
                                    span.clone(),
                                    *basic_block,
                                    "write",
                                    place_ty.clone(),
                                ));
                            }
                            _ => {}
                        },
                        _ => {
                            log::error!("State transition edge must be transition!");
                        }
                    }
                }
            }

            let state_node = self.state_graph.graph.node_weight(state).unwrap();
            let state_mark = state_node.mark.clone();
            // Check for data races in the state
            self.check_race_in_state(&state_transitions, state_mark, &mut race_infos);
        }

        // Merge similar race conditions
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
        transitions: &[(NodeIndex, String, usize, &str, String)],
        state_marks: Vec<(usize, u8)>,
        race_infos: &mut Vec<RaceCondition>,
    ) {
        // Traverse all transition pairs
        for (i, (node_idx1, span1, bb1, op_type1, data1_ty)) in transitions.iter().enumerate() {
            for (node_idx2, span2, bb2, op_type2, data2_ty) in transitions.iter().skip(i + 1) {
                // Check if accessing the same variable (same NodeIndex)
                if node_idx1 == node_idx2 {
                    // Constitutes a race only when at least one is a write operation
                    if *op_type1 == "write" || *op_type2 == "write" {
                        let operations = vec![
                            RaceOperation {
                                operation_type: op_type1.to_string(),
                                variable: data1_ty.clone(),
                                location: span1.clone(),
                                basic_block: Some(*bb1),
                            },
                            RaceOperation {
                                operation_type: op_type2.to_string(),
                                variable: data2_ty.clone(),
                                location: span2.clone(),
                                basic_block: Some(*bb2),
                            },
                        ];

                        race_infos.push(RaceCondition {
                            operations,
                            variable_info: format!("Race in varible: {}", node_idx1.index()),
                            state: state_marks.clone(),
                        });
                    }
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
}
