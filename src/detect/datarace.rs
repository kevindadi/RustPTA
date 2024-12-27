use crate::graph::cpn::{ColorPetriNode, DataOpType};
use crate::graph::cpn_state_graph::{CpnStateGraph, RaceInfo};
use crate::memory::pointsto::AliasId;
use crate::report::{RaceCondition, RaceOperation, RaceReport, VariableInfo};
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub struct DataRaceDetector<'a> {
    state_graph: &'a CpnStateGraph,
}

impl<'a> DataRaceDetector<'a> {
    pub fn new(state_graph: &'a CpnStateGraph) -> Self {
        Self { state_graph }
    }

    pub fn detect(&self) -> RaceReport {
        let start_time = Instant::now();
        let mut report = RaceReport::new("CPN Data Race Detector".to_string());

        // 获取所有使能的变迁
        let enabled_transitions = self.get_enabled_transitions();

        // 使用 CpnStateGraph 的检测方法
        let race_infos = self.state_graph.detect_race_condition(&enabled_transitions);

        // 将 RaceInfo 转换为 RaceCondition
        let race_conditions = self.convert_race_infos(race_infos);

        if !race_conditions.is_empty() {
            report.has_race = true;
            report.race_count = race_conditions.len();
            report.race_conditions = race_conditions;
        }

        report.analysis_time = start_time.elapsed();
        report
    }

    /// 获取当前所有使能的变迁
    fn get_enabled_transitions(&self) -> Vec<NodeIndex> {
        let mut enabled = Vec::new();

        // 遍历所有状态
        for state in self.state_graph.graph.node_indices() {
            for edge in self.state_graph.graph.edges(state) {
                let transition = edge.weight().transition;
                if let Some(ColorPetriNode::UnsafeTransition { .. }) =
                    self.state_graph.initial_net.node_weight(transition)
                {
                    enabled.push(transition);
                }
            }
        }

        enabled
    }

    /// 将 RaceInfo 转换为 RaceCondition
    fn convert_race_infos(&self, race_infos: HashSet<RaceInfo>) -> Vec<RaceCondition> {
        let mut race_conditions = Vec::new();
        let mut data_info_cache: HashMap<AliasId, VariableInfo> = HashMap::new();

        for race_info in race_infos {
            // 收集所有相关的操作信息
            let mut operations = Vec::new();
            for (transition_idx, span) in race_info.transitions.iter().zip(race_info.span.iter()) {
                if let Some(ColorPetriNode::UnsafeTransition {
                    data_ops,
                    rw_type,
                    info,
                    ..
                }) = self
                    .state_graph
                    .initial_net
                    .node_weight(NodeIndex::new(*transition_idx))
                {
                    operations.push(RaceOperation {
                        operation_type: match rw_type {
                            DataOpType::Read => "read",
                            DataOpType::Write => "write",
                        }
                        .to_string(),
                        thread_name: format!("Thread-{}", transition_idx),
                        variable: format!(
                            "Var_{}_{}",
                            data_ops.instance_id.index(),
                            data_ops.local.index()
                        ),
                        location: span.clone(),
                        basic_block: None, // 可以从 UnsafeTransition 中获取
                    });

                    // 缓存变量信息
                    if !data_info_cache.contains_key(data_ops) {
                        data_info_cache.insert(
                            data_ops.clone(),
                            VariableInfo {
                                name: info.clone(),
                                data_type: "Shared Memory".to_string(),
                                function_scope: format!(
                                    "Function_{}",
                                    data_ops.instance_id.index()
                                ),
                            },
                        );
                    }
                }
            }

            // 创建竞争条件
            if let Some(variable_info) = data_info_cache.get(&AliasId {
                instance_id: NodeIndex::new(race_info.unsafe_data.data_func),
                local: race_info.unsafe_data.data_local.into(),
            }) {
                race_conditions.push(RaceCondition {
                    operations,
                    variable_info: variable_info.clone(),
                    state: None, // 可以添加状态信息如果需要
                });
            }
        }

        race_conditions
    }
}
