use crate::graph::pn::{CallType, ControlType, PetriNetNode};
use crate::graph::state_graph::StateGraph;
use crate::report::{AtomicOperation, AtomicReport, AtomicViolation};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub struct AtomicityViolationDetector<'a> {
    state_graph: &'a StateGraph,
}

impl<'a> AtomicityViolationDetector<'a> {
    pub fn new(state_graph: &'a StateGraph) -> Self {
        Self { state_graph }
    }

    pub fn detect(&self) -> AtomicReport {
        let start_time = Instant::now();
        let mut report = AtomicReport::new("Petri Net Atomicity Violation Detector".to_string());

        // 检测未同步的路径违背
        let path_violations = self.detect_unsynchronized_path_violations();

        // 检测并发 Relaxed 操作违背
        let concurrent_violations = self.detect_concurrent_relaxed_violations();

        // 合并所有违背
        let all_violations: Vec<_> = path_violations
            .into_iter()
            .chain(concurrent_violations.into_iter())
            .collect();

        if !all_violations.is_empty() {
            report.has_violation = true;
            report.violation_count = all_violations.len();
            report.violations = all_violations;
        }

        report.analysis_time = start_time.elapsed();
        report
    }

    /// 检测未同步的路径违背
    fn detect_unsynchronized_path_violations(&self) -> Vec<AtomicViolation> {
        let mut violations = Vec::new();

        for state in self.state_graph.graph.node_indices() {
            for edge in self.state_graph.graph.edges(state) {
                let transition = edge.weight().transition;
                if let Some(PetriNetNode::T(t)) =
                    self.state_graph.initial_net.node_weight(transition)
                {
                    if let ControlType::Call(CallType::AtomicLoad(var_id, ordering, span)) =
                        &t.transition_type
                    {
                        // 检查前向路径
                        let (forward_violation, forward_ops) = self.check_path_violation(
                            state,
                            var_id,
                            Direction::Incoming,
                            span.clone(),
                        );

                        // 检查后向路径
                        let (backward_violation, backward_ops) = self.check_path_violation(
                            state,
                            var_id,
                            Direction::Outgoing,
                            span.clone(),
                        );

                        if forward_violation || backward_violation {
                            let mut operations = vec![AtomicOperation {
                                operation_type: "load".to_string(),
                                ordering: format!("{:?}", ordering),
                                variable: format!("{:?}", var_id),
                                location: span.clone(),
                            }];

                            operations.extend(forward_ops);
                            operations.extend(backward_ops);

                            violations.push(AtomicViolation {
                                violation_type: "unsynchronized_path".to_string(),
                                operations,
                                state: Some(self.state_graph.graph[state].mark.clone()),
                                path: None,
                            });
                        }
                    }
                }
            }
        }

        violations
    }

    /// 检测并发 Relaxed 操作违背
    fn detect_concurrent_relaxed_violations(&self) -> Vec<AtomicViolation> {
        let mut violations = Vec::new();

        // 遍历所有状态
        for state in self.state_graph.graph.node_indices() {
            let mut relaxed_ops: HashMap<_, Vec<(String, String, String)>> = HashMap::new();

            // 收集当前状态下所有可能的 Relaxed 操作
            for edge in self.state_graph.graph.edges(state) {
                if let Some(PetriNetNode::T(t)) = self
                    .state_graph
                    .initial_net
                    .node_weight(edge.weight().transition)
                {
                    match &t.transition_type {
                        ControlType::Call(CallType::AtomicLoad(var_id, ordering, span))
                        | ControlType::Call(CallType::AtomicStore(var_id, ordering, span)) => {
                            if format!("{:?}", ordering) == "Relaxed" {
                                relaxed_ops.entry(var_id.clone()).or_default().push((
                                    if matches!(
                                        &t.transition_type,
                                        ControlType::Call(CallType::AtomicLoad(..))
                                    ) {
                                        "load"
                                    } else {
                                        "store"
                                    }
                                    .to_string(),
                                    "Relaxed".to_string(),
                                    span.clone(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 检查每个变量的并发操作
            for (var_id, ops) in relaxed_ops {
                if ops.len() >= 2 {
                    let operations = ops
                        .into_iter()
                        .map(|(op_type, ordering, location)| AtomicOperation {
                            operation_type: op_type,
                            ordering,
                            variable: format!("{:?}", var_id),
                            location,
                        })
                        .collect();

                    violations.push(AtomicViolation {
                        violation_type: "concurrent_relaxed_operations".to_string(),
                        operations,
                        state: Some(self.state_graph.graph[state].mark.clone()),
                        path: None,
                    });
                }
            }
        }

        violations
    }

    /// 检查路径上是否存在未同步的违背
    fn check_path_violation(
        &self,
        state: NodeIndex,
        var_id: &crate::memory::pointsto::AliasId,
        direction: Direction,
        span: String,
    ) -> (bool, Vec<AtomicOperation>) {
        let mut visited = HashSet::new();
        let mut operations = Vec::new();
        let mut has_violation = false;

        let mut stack = vec![state];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }

            for edge in self.state_graph.graph.edges_directed(current, direction) {
                let next = match direction {
                    Direction::Incoming => edge.source(),
                    Direction::Outgoing => edge.target(),
                };

                if let Some(PetriNetNode::T(t)) = self
                    .state_graph
                    .initial_net
                    .node_weight(edge.weight().transition)
                {
                    match &t.transition_type {
                        ControlType::Call(CallType::AtomicStore(v_id, ordering, store_span)) => {
                            if v_id == var_id {
                                has_violation = true;
                                operations.push(AtomicOperation {
                                    operation_type: "store".to_string(),
                                    ordering: format!("{:?}", ordering),
                                    variable: format!("{:?}", var_id),
                                    location: store_span.clone(),
                                });
                            }
                        }
                        ControlType::Call(CallType::Lock(_)) | ControlType::Drop(_) => {
                            // 发现同步操作，停止搜索这条路径
                            continue;
                        }
                        _ => {}
                    }
                }

                stack.push(next);
            }
        }

        (has_violation, operations)
    }
}
