use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Graph;

use std::hash::Hash;
use std::hash::Hasher;

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

#[derive(Debug, Clone, PartialEq)]
pub struct StateNode {
    pub mark: Vec<(NodeIndex, usize)>,
}

impl Hash for StateNode {
    fn hash<H: Hasher>(&self, state_node: &mut H) {
        // self.mark.sort();
        self.mark.hash(state_node);
    }
}

impl StateNode {
    pub fn new(mark: Vec<(NodeIndex, usize)>) -> Self {
        Self { mark }
    }
}

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
        use petgraph::dot::Dot;
        use std::io::Write;
        let mut sg_file = std::fs::File::create("sg.dot").unwrap();
    }
}
