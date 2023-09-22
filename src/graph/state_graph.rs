use crate::graph::petri_net::Place;

#[derive(Debug, Clone)]
pub struct State {
    mark: Vec<Place>,
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let places = self
            .mark
            .iter()
            .map(|place| format!("{}", place))
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "State {{ mark: [{}] }}", places)
    }
}

impl State {
    pub fn new(mark: Vec<Place>) -> Self {
        Self { mark }
    }
}
