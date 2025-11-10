use std::collections::HashSet;

use crate::net::ids::{PlaceId, TransitionId};
use crate::net::structure::PlaceType;

use super::graph::ReductionGraph;
use super::ReductionStep;

impl ReductionGraph {
    pub(crate) fn remove_simple_loops(&mut self) -> Vec<ReductionStep> {
        let mut steps = Vec::new();
        let mut visited = HashSet::new();

        for start_idx in 0..self.places.len() {
            if visited.contains(&start_idx) {
                continue;
            }
            if self.places[start_idx].removed {
                continue;
            }
            if self.places[start_idx].place.place_type == PlaceType::Resources {
                continue;
            }
            if self.places[start_idx].outgoing.len() != 1 {
                continue;
            }
            if self.places[start_idx].incoming.len() != 1 {
                continue;
            }

            let mut cycle_places = Vec::new();
            let mut cycle_transitions = Vec::new();

            let mut current_place = start_idx;
            let mut local_visited = HashSet::new();
            let mut valid_cycle = true;

            loop {
                if !local_visited.insert(current_place) {
                    valid_cycle = false;
                    break;
                }
                let transition_idx = match self.places[current_place].outgoing.first() {
                    Some(idx) => *idx,
                    None => {
                        valid_cycle = false;
                        break;
                    }
                };
                if self.transitions[transition_idx].removed {
                    valid_cycle = false;
                    break;
                }
                if self.transitions[transition_idx].inputs.len() != 1
                    || self.transitions[transition_idx].outputs.len() != 1
                {
                    valid_cycle = false;
                    break;
                }

                let next_place = self.transitions[transition_idx].outputs[0].0;

                if self.places[next_place].removed {
                    valid_cycle = false;
                    break;
                }
                if self.places[next_place].place.place_type == PlaceType::Resources {
                    valid_cycle = false;
                    break;
                }
                if self.places[next_place].incoming.len() != 1
                    || self.places[next_place].outgoing.len() != 1
                {
                    valid_cycle = false;
                    break;
                }
                cycle_places.push(current_place);
                cycle_transitions.push(transition_idx);

                if next_place == start_idx {
                    break;
                }
                current_place = next_place;
            }

            if !valid_cycle {
                continue;
            }

            let all_tokens_zero = cycle_places
                .iter()
                .all(|idx| self.places[*idx].place.tokens == 0);
            if !all_tokens_zero {
                continue;
            }

            for place_idx in &cycle_places {
                visited.insert(*place_idx);
            }

            let removed_places: Vec<PlaceId> = cycle_places
                .iter()
                .flat_map(|idx| self.places[*idx].originals.clone())
                .collect();
            let removed_transitions: Vec<TransitionId> = cycle_transitions
                .iter()
                .flat_map(|idx| self.transitions[*idx].originals.clone())
                .collect();

            for transition_idx in &cycle_transitions {
                self.remove_transition(*transition_idx);
            }
            for place_idx in &cycle_places {
                self.remove_place(*place_idx);
            }
            self.clean_adjacency();

            if !removed_places.is_empty() || !removed_transitions.is_empty() {
                steps.push(ReductionStep::LoopRemoved {
                    removed_places,
                    removed_transitions,
                });
            }
        }

        steps
    }
}
