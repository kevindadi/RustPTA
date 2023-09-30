use crate::graph::petri_net::Place;
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct StateEdge {
    label: String,
    weight: u32,
}

impl StateEdge {
    pub fn new(label: String, weight: u32) -> Self {
        Self { label, weight }
    }
}

#[derive(Debug, Clone)]
pub struct StateNode {
    pub mark: HashSet<NodeIndex>,
}

// impl std::fmt::Display for StateNode {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let places = self
//             .mark
//             .iter()
//             .map(|place| format!("{}", place))
//             .collect::<Vec<_>>()
//             .join(",");
//         write!(f, "State {{ mark: [{}] }}", places)
//     }
// }

impl StateNode {
    pub fn new(mark: HashSet<NodeIndex>) -> Self {
        Self { mark }
    }
}

#[derive(Debug, Clone)]
pub struct StateGraph {
    pub graph: Graph<StateNode, StateEdge>,
}

impl StateGraph {
    pub fn new() -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
        }
    }

    /// Print the stategraph in dot format.
    #[allow(dead_code)]
    pub fn dot(&self) {
        println!(
            "{:?}",
            Dot::with_config(&self.graph, &[Config::GraphContentOnly])
        );
    }
}
