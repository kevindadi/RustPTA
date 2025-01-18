use crate::graph::callgraph::InstanceId;
use crate::graph::pn::{CallType, ControlType, PetriNetEdge, PetriNetNode};
use crate::graph::state_graph::{StateEdge, StateGraph, StateNode};
use crate::memory::pointsto::AliasId;
use crate::report::{AtomicOperation, AtomicReport, AtomicViolation, ViolationPattern};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

pub struct AtomicityViolationDetector<'a> {
    state_graph: &'a StateGraph,
}

#[derive(Clone)]
struct AtomicOp {
    var_id: AliasId,
    ordering: String,
    span: String,
    thread_id: InstanceId,
}

impl<'a> AtomicityViolationDetector<'a> {
    pub fn new(state_graph: &'a StateGraph) -> Self {
        Self { state_graph }
    }

    pub fn detect(&self) -> AtomicReport {
        let start_time = Instant::now();
        let mut report = AtomicReport::new("Petri Net Atomicity Violation Detector".to_string());

        // 1. 收集所有的 load 和 store 操作
        let (loads, stores) = self.collect_atomic_operations();

        // 2. 将图结构包装在 Arc<RwLock> 中
        let graph = Arc::new(RwLock::new(self.state_graph.graph.clone()));
        let initial_net = Arc::new(RwLock::new(self.state_graph.initial_net.as_ref().clone()));

        // 3. 并行检查违背
        let violations = self.check_violations(loads, stores, &graph, &initial_net);

        if !violations.is_empty() {
            report.has_violation = true;
            report.violation_count = violations.len();
            report.violations = violations;
        }

        report.analysis_time = start_time.elapsed();
        report
    }

    pub fn generate_atomic_races(&self) -> String {
        let mut report = String::new();
        report.push_str("Found Atomic Race Conditions:\n\n");

        for (i, info) in self.state_graph.atomic_races.iter().enumerate() {
            report.push_str(&format!("Race Condition #{}\n", i + 1));
            report.push_str(&format!("State: {:?}\n", info.state));
            report.push_str("Conflicting Operations:\n");

            for op in &info.operations {
                report.push_str(&format!(
                    "- {} operation at {}, ordering: {}\n",
                    match op.op_type {
                        AtomicOpType::Load => "Load",
                        AtomicOpType::Store => "Store",
                    },
                    op.span,
                    op.ordering
                ));
            }
            report.push_str("\n");
        }

        report
    }

    fn collect_atomic_operations(
        &self,
    ) -> (HashMap<NodeIndex, AtomicOp>, HashMap<NodeIndex, AtomicOp>) {
        let mut loads = HashMap::new();
        let mut stores = HashMap::new();

        for node_idx in self.state_graph.initial_net.node_indices() {
            if let Some(PetriNetNode::T(t)) = self.state_graph.initial_net.node_weight(node_idx) {
                match &t.transition_type {
                    ControlType::Call(CallType::AtomicLoad(var_id, ordering, span, thread_id)) => {
                        if format!("{:?}", ordering) == "Relaxed" {
                            loads.insert(
                                node_idx,
                                AtomicOp {
                                    var_id: var_id.clone(),
                                    ordering: format!("{:?}", ordering),
                                    span: span.clone(),
                                    thread_id: *thread_id,
                                },
                            );
                        }
                    }
                    ControlType::Call(CallType::AtomicStore(var_id, ordering, span, thread_id)) => {
                        stores.insert(
                            node_idx,
                            AtomicOp {
                                var_id: var_id.clone(),
                                ordering: format!("{:?}", ordering),
                                span: span.clone(),
                                thread_id: *thread_id,
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        (loads, stores)
    }

    fn check_violations(
        &self,
        loads: HashMap<NodeIndex, AtomicOp>,
        stores: HashMap<NodeIndex, AtomicOp>,
        graph: &Arc<RwLock<Graph<StateNode, StateEdge>>>,
        initial_net: &Arc<RwLock<Graph<PetriNetNode, PetriNetEdge>>>,
    ) -> Vec<ViolationPattern> {
        let all_violations = Arc::new(Mutex::new(Vec::new()));
        let mut handles = vec![];

        for (load_trans, load_op) in loads {
            let all_violations = Arc::clone(&all_violations);
            let stores = stores.clone();
            let graph = Arc::clone(graph);
            let initial_net = Arc::clone(initial_net);
            unsafe {
                let handle = thread::spawn(move || {
                    let graph = graph.read().unwrap();
                    for state in graph.node_indices() {
                        for edge in graph.edges_directed(state, Direction::Outgoing) {
                            if edge.weight().transition == load_trans {
                                if let Some(violation) = Self::check_state_for_violation(
                                    &graph,
                                    &initial_net.read().unwrap(),
                                    state,
                                    &load_op,
                                    &stores,
                                ) {
                                    all_violations.lock().unwrap().push(violation);
                                }
                            }
                        }
                    }
                });
                handles.push(handle);
            }
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let violations = Arc::try_unwrap(all_violations)
            .unwrap()
            .into_inner()
            .unwrap();
        let mut pattern_map: HashMap<ViolationPattern, Vec<Vec<(usize, u8)>>> = HashMap::new();

        for violation in violations {
            let pattern = ViolationPattern {
                load_op: AtomicOperation {
                    operation_type: "load".to_string(),
                    ordering: violation.pattern.load_op.ordering.clone(),
                    variable: violation.pattern.load_op.variable.clone(),
                    location: violation.pattern.load_op.location.clone(),
                },
                store_ops: violation.pattern.store_ops.clone(),
            };

            let mut states = violation.states.clone();
            states.sort();
            pattern_map.entry(pattern).or_default().push(states.clone());
        }

        pattern_map
            .into_iter()
            .map(|(pattern, _)| pattern)
            .collect()
    }

    fn check_state_for_violation(
        graph: &Graph<StateNode, StateEdge>,
        initial_net: &Graph<PetriNetNode, PetriNetEdge>,
        load_state: NodeIndex,
        load_op: &AtomicOp,
        stores: &HashMap<NodeIndex, AtomicOp>,
    ) -> Option<AtomicViolation> {
        let mut visited = HashSet::new();
        let mut write_operations = HashSet::new();
        let mut stack = vec![load_state];

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }

            for edge in graph.edges_directed(current, Direction::Incoming) {
                let source = edge.source();
                let transition = edge.weight().transition;

                if let Some(PetriNetNode::T(t)) = initial_net.node_weight(transition) {
                    if let ControlType::Start(thread_id) = t.transition_type {
                        if thread_id == load_op.thread_id {
                            break;
                        }
                    }
                }

                if let Some(store_op) = stores.get(&transition) {
                    if store_op.var_id == load_op.var_id {
                        write_operations.insert(AtomicOperation {
                            operation_type: "store".to_string(),
                            ordering: store_op.ordering.clone(),
                            variable: format!("{:?}", store_op.var_id),
                            location: store_op.span.clone(),
                        });
                    }
                }

                stack.push(source);
            }
        }

        let mut write_operations: Vec<_> = write_operations.into_iter().collect();
        if write_operations.len() >= 2 {
            // 对 store 操作进行排序以保证相同集合有相同的顺序
            write_operations.sort_by(|a, b| a.location.cmp(&b.location));

            Some(AtomicViolation {
                pattern: ViolationPattern {
                    load_op: AtomicOperation {
                        operation_type: "load".to_string(),
                        ordering: load_op.ordering.clone(),
                        variable: format!("{:?}", load_op.var_id),
                        location: load_op.span.clone(),
                    },
                    store_ops: write_operations,
                },
                states: graph[load_state].mark.clone(),
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct AtomicRaceInfo {
    pub state: Vec<(usize, u8)>,              // 发生竞争的状态
    pub operations: Vec<AtomicRaceOperation>, // 冲突的操作
}

#[derive(Debug, Clone)]
pub struct AtomicRaceOperation {
    pub op_type: AtomicOpType,
    pub transition: NodeIndex,
    pub var_id: AliasId,
    pub ordering: String,
    pub span: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AtomicOpType {
    Load,
    Store,
}
