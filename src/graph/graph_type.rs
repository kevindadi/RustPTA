use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::graph::petri_net::{PetriNet, PetriNetNode};

pub trait OutputType {
    fn lola(&self) -> String;

    fn pnml(&self);
}

impl<'compilation, 'tcx, 'a> OutputType for PetriNet<'compilation, 'tcx, 'a> {
    fn lola(&self) -> String {
        let mut places = Vec::<String>::new();
        let mut transitions = Vec::<String>::new();
        let mut markings = Vec::<String>::new();

        for index in self.net.node_indices() {
            match &self.net[index] {
                PetriNetNode::P(place) => {
                    places.push(place.name.clone());
                    if *place.tokens.borrow() > 0 {
                        markings.push(format!(
                            "{}: {}",
                            place.name,
                            place.tokens.borrow().to_owned()
                        ));
                    }
                }
                PetriNetNode::T(transition) => {
                    let mut consume = Vec::new();
                    let mut produce = Vec::new();

                    for edge in self.net.edges_directed(index, Direction::Incoming) {
                        if let PetriNetNode::P(place) =
                            &self.net.node_weight(edge.source()).unwrap()
                        {
                            consume.push(format!(
                                "{}: {}",
                                place.name.clone(),
                                edge.weight().label
                            ));
                        }
                    }

                    for edge in self.net.edges_directed(index, Direction::Outgoing) {
                        if let PetriNetNode::P(place) =
                            &self.net.node_weight(edge.target()).unwrap()
                        {
                            produce.push(format!(
                                "{}: {}",
                                place.name.clone(),
                                edge.weight().label
                            ));
                        }
                    }

                    transitions.push(format!(
                        "TRANSITION {}\nCONSUME {};\nPRODUCE {};\n",
                        transition.name.clone(),
                        consume.join(", "),
                        produce.join(", ")
                    ));
                }
            }
        }

        format!(
            "PLACE\n{}\n;\n\nMARKING\n{}\n;\n\n{}",
            places.join(", "),
            markings.join(", "),
            transitions.join("\n")
        )
    }

    fn pnml(&self) {}
}
