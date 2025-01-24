use crate::graph::net_structure::{ControlType, PetriNetNode};
use crate::graph::state_graph::{StateGraph, StateNode};
use crate::report::{RaceCondition, RaceOperation, RaceReport};
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
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

        // 遍历所有状态节点
        for state in self.state_graph.graph.node_indices() {
            let mut state_transitions = Vec::new();

            // 收集当前状态的所有不安全操作
            for edge in self.state_graph.graph.edges(state) {
                let transition = edge.weight().transition;
                if let Some(node) = self.state_graph.initial_net.node_weight(transition) {
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
            // 检查状态中的数据竞争
            self.check_race_in_state(&state_transitions, state_mark, &mut race_infos);
        }

        // 合并相似的竞争条件
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
        // 遍历所有变迁对
        for (i, (node_idx1, span1, bb1, op_type1, data1_ty)) in transitions.iter().enumerate() {
            for (node_idx2, span2, bb2, op_type2, data2_ty) in transitions.iter().skip(i + 1) {
                // 检查是否访问相同的变量（NodeIndex相同）
                if node_idx1 == node_idx2 {
                    // 至少有一个是写操作时才构成竞争
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
                    // 合并操作，保留唯一的操作
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
