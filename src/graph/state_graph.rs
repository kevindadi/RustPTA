use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
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

// impl std::fmt::Display for StateNode {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let places = self
//             .mark
//             .iter()
//             .map(|place| format!("{}", place.0.index()))
//             .collect::<Vec<_>>()
//             .join(",");
//         write!(f, "State {{ mark: [{}] }}", places)
//     }
// }

impl StateNode {
    pub fn new(mark: Vec<(NodeIndex, usize)>) -> Self {
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

// struct StateGraphDot;

// impl petgraph::dot::Dot for StateGraphDot {
//     fn dot_header<W>(&self, write: &mut W) -> std::io::Result<()>
//     where
//         W: std::io::Write,
//     {
//         write!(write, "digraph StateGraph {{")?;
//         Ok(())
//     }
// }
