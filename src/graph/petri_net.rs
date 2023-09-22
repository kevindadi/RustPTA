use petgraph::dot::{Config, Dot};
use petgraph::Graph;

use super::state_graph::State;

#[derive(Debug, Clone)]
pub struct Place {
    name: String,
    token: u32,
}

impl Place {
    pub fn new(name: String, token: u32) -> Self {
        Self { name, token }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Transition {
    name: String,
    time: (u32, u32),
    weight: u32,
}

impl Transition {
    pub fn new(name: String, time: (u32, u32), weight: u32) -> Self {
        Self { name, time, weight }
    }
}

#[derive(Debug, Clone)]
pub enum PetriNetNode {
    P(Place),
    T(Transition),
}

impl std::fmt::Display for PetriNetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PetriNetNode::P(place) => write!(f, "{}", place.name),
            PetriNetNode::T(transition) => write!(f, "{}", transition.name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PetriNetEdge {
    label: String,
}

impl std::fmt::Display for PetriNetEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

pub struct PetriNet {
    net: Graph<PetriNetNode, PetriNetEdge>,
}

// impl std::fmt::Display for PetriNet {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let config = Config::default()
//             .node_shape(|node, _| match &self.net[node] {
//                 PetriNetNode::P(_) => "circle",
//                 PetriNetNode::T(_) => "rectangle",
//             })
//             .node_style(|_, _| "filled")
//             .edge_style(|_, _| "solid");

//         write!(f, "{}", Dot::with_config(&self.net, &[config]))
//     }
// }

impl PetriNet {
    pub fn new() -> Self {
        Self {
            net: Graph::<PetriNetNode, PetriNetEdge>::new(),
        }
    }

    // Get initial state of Petri net.
    // pub fn get_initial_state(&self) -> State {
    //     let mark = self
    //         .net
    //         .node_indices()
    //         .filter_map(|idx| match &self.net[idx] {
    //             PetriNetNode::P(Place { token: 0, .. }) => Some(&self.net[idx]),
    //             _ => None,
    //         })
    //         .collect();
    //     State::new(mark)
    // }

    // Get all enabled transitions at current state
    pub fn get_sched_transitions(&self) {}

    // Choose a transition to fire
    pub fn fire_transition(&mut self) {}

    // Generate state graph for Petri net
    pub fn generate_state_graph(&mut self) {}
}
