//! 运行时: 可发生集、发生语义与冲突检测定义。
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::fmt;

use thiserror::Error;

use crate::net::ids::{PlaceId, TransitionId};
use crate::net::incidence::Incidence;
#[cfg(feature = "inhibitor")]
use crate::net::incidence::IncidenceBool;
use crate::net::index_vec::{Idx, IndexVec};
use crate::net::structure::{Marking, Place, Transition, Weight};

#[cfg(feature = "invariants")]
use num::bigint::BigInt;
#[cfg(feature = "invariants")]
use num::integer::Integer;
#[cfg(feature = "invariants")]
use num::rational::BigRational;
#[cfg(feature = "invariants")]
use num::traits::{One, Signed, Zero};

#[derive(Debug, Error)]
pub enum FireError {
    #[error("transition {0:?} is out of bounds")]
    OutOfBounds(TransitionId),
    #[error("transition {0:?} is not enabled under the supplied marking")]
    NotEnabled(TransitionId),
    #[error("transition {transition:?} conflicts on place {place:?}")]
    Conflict {
        transition: TransitionId,
        place: PlaceId,
    },
    #[error("capacity exceeded at place {place:?}: {after} > {capacity}")]
    Capacity {
        place: PlaceId,
        after: Weight,
        capacity: Weight,
    },
    #[error("no enabled transitions under the supplied marking")]
    Deadlock,
    #[error("fire_plan contains non-sequential step with {0} transitions")]
    NonSequentialStep(usize),
}

#[derive(Debug, Clone)]
pub struct ReachabilityEdge {
    pub source: usize,
    pub transition: TransitionId,
    pub target: usize,
}

#[derive(Debug, Clone)]
pub struct ReachabilityGraph {
    pub markings: Vec<Marking>,
    pub edges: Vec<ReachabilityEdge>,
    pub deadlocks: Vec<usize>,
    pub truncated: bool,
}

impl ReachabilityGraph {
    pub fn new() -> Self {
        Self {
            markings: Vec::new(),
            edges: Vec::new(),
            deadlocks: Vec::new(),
            truncated: false,
        }
    }

    pub fn add_marking(&mut self, marking: Marking) -> usize {
        let idx = self.markings.len();
        self.markings.push(marking);
        idx
    }

    pub fn add_edge(&mut self, source: usize, transition: TransitionId, target: usize) {
        self.edges.push(ReachabilityEdge {
            source,
            transition,
            target,
        });
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Net {
    pub places: IndexVec<PlaceId, Place>,
    pub transitions: IndexVec<TransitionId, Transition>,
    pub pre: Incidence<u64>,
    pub post: Incidence<u64>,
    pub capacity: Option<IndexVec<PlaceId, Weight>>,
    #[cfg(feature = "inhibitor")]
    pub inhibitor: Option<IncidenceBool>,
    #[cfg(feature = "reset")]
    pub reset: Option<IncidenceBool>,
}

impl fmt::Debug for Net {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Net")
            .field("places", &self.places)
            .field("transitions", &self.transitions)
            .field("pre", &self.pre)
            .field("post", &self.post)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl Net {
    pub fn empty() -> Self {
        Self {
            places: IndexVec::new(),
            transitions: IndexVec::new(),
            pre: Incidence::new(0, 0, 0u64),
            post: Incidence::new(0, 0, 0u64),
            capacity: None,
            #[cfg(feature = "inhibitor")]
            inhibitor: None,
            #[cfg(feature = "reset")]
            reset: None,
        }
    }

    pub fn new(
        places: IndexVec<PlaceId, Place>,
        transitions: IndexVec<TransitionId, Transition>,
        pre: Incidence<u64>,
        post: Incidence<u64>,
        capacity: Option<IndexVec<PlaceId, Weight>>,
        #[cfg(feature = "inhibitor")] inhibitor: Option<IncidenceBool>,
        #[cfg(feature = "reset")] reset: Option<IncidenceBool>,
    ) -> Self {
        Self {
            places,
            transitions,
            pre,
            post,
            capacity,
            #[cfg(feature = "inhibitor")]
            inhibitor,
            #[cfg(feature = "reset")]
            reset,
        }
    }

    pub fn add_place(&mut self, place: Place) -> PlaceId {
        let capacity_value = place.capacity;
        let place_id = self.places.push(place);
        self.pre.push_place_with_default(0);
        self.post.push_place_with_default(0);

        if let Some(capacity_vec) = self.capacity.as_mut() {
            capacity_vec.push(capacity_value);
        }
        #[cfg(feature = "inhibitor")]
        if let Some(inhibitor) = self.inhibitor.as_mut() {
            inhibitor.push_place();
        }
        #[cfg(feature = "reset")]
        if let Some(reset) = self.reset.as_mut() {
            reset.push_place();
        }
        place_id
    }

    pub fn add_transition(&mut self, transition: Transition) -> TransitionId {
        let transition_id = self.transitions.push(transition);
        self.pre.push_transition_with_default(0);
        self.post.push_transition_with_default(0);
        #[cfg(feature = "inhibitor")]
        if let Some(inhibitor) = self.inhibitor.as_mut() {
            inhibitor.push_transition();
        }
        #[cfg(feature = "reset")]
        if let Some(reset) = self.reset.as_mut() {
            reset.push_transition();
        }
        transition_id
    }

    pub fn set_input_weight(&mut self, place: PlaceId, transition: TransitionId, weight: Weight) {
        self.pre.set(place, transition, weight);
    }

    pub fn set_output_weight(&mut self, place: PlaceId, transition: TransitionId, weight: Weight) {
        self.post.set(place, transition, weight);
    }

    pub fn add_input_arc(&mut self, place: PlaceId, transition: TransitionId, weight: Weight) {
        if weight == 0 {
            return;
        }
        let entry = self.pre.get_mut(place, transition);
        *entry += weight;
    }

    pub fn add_output_arc(&mut self, place: PlaceId, transition: TransitionId, weight: Weight) {
        if weight == 0 {
            return;
        }
        let entry = self.post.get_mut(place, transition);
        *entry += weight;
    }

    #[cfg(feature = "inhibitor")]
    pub fn set_inhibitor_arc(&mut self, place: PlaceId, transition: TransitionId, value: bool) {
        if self.inhibitor.is_none() {
            self.inhibitor = Some(IncidenceBool::new(
                self.pre.places(),
                self.pre.transitions(),
            ));
        }
        if let Some(matrix) = self.inhibitor.as_mut() {
            matrix.set(place, transition, value);
        }
    }

    #[cfg(feature = "reset")]
    pub fn set_reset_arc(&mut self, place: PlaceId, transition: TransitionId, value: bool) {
        if self.reset.is_none() {
            self.reset = Some(IncidenceBool::new(
                self.pre.places(),
                self.pre.transitions(),
            ));
        }
        if let Some(matrix) = self.reset.as_mut() {
            matrix.set(place, transition, value);
        }
    }

    pub fn places_len(&self) -> usize {
        self.places.len()
    }

    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }

    pub fn initial_marking(&self) -> Marking {
        Marking(IndexVec::from(
            self.places.iter().map(|p| p.tokens).collect::<Vec<_>>(),
        ))
    }

    pub fn incidence(&self) -> (&Incidence<u64>, &Incidence<u64>) {
        (&self.pre, &self.post)
    }

    pub fn c_matrix(&self) -> Incidence<i64> {
        self.post.difference(&self.pre)
    }

    pub fn enabled_transitions(&self, marking: &Marking) -> Vec<TransitionId> {
        self.transitions
            .iter()
            .enumerate()
            .filter_map(|(idx, _)| {
                let transition = TransitionId::from_usize(idx);
                if self.is_transition_enabled(transition, marking) {
                    Some(transition)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn fire_transition(
        &self,
        marking: &Marking,
        transition: TransitionId,
    ) -> Result<Marking, FireError> {
        if transition.index() >= self.transitions_len() {
            return Err(FireError::OutOfBounds(transition));
        }
        if !self.is_transition_enabled(transition, marking) {
            return Err(FireError::NotEnabled(transition));
        }

        let mut next = marking.clone();

        for (place, _) in self.places.iter_enumerated() {
            let weight = *self.pre.get(place, transition);
            if weight > 0 {
                let tokens = next.tokens_mut(place);
                *tokens = tokens
                    .checked_sub(weight)
                    .expect("enabled transition must have sufficient tokens");
            }
        }

        for (place, _) in self.places.iter_enumerated() {
            let weight = *self.post.get(place, transition);
            if weight > 0 {
                let tokens = next.tokens_mut(place);
                let after = *tokens + weight;
                let capacity = self
                    .capacity
                    .as_ref()
                    .map(|caps| caps[place])
                    .unwrap_or(self.places[place].capacity);
                if after > capacity {
                    return Err(FireError::Capacity {
                        place,
                        after,
                        capacity,
                    });
                }
                *tokens = after;
            }
        }

        #[cfg(feature = "reset")]
        if let Some(reset) = self.reset.as_ref() {
            for (place, _) in self.places.iter_enumerated() {
                if reset.get(place, transition) {
                    *next.tokens_mut(place) = 0;
                }
            }
        }

        Ok(next)
    }

    pub fn reachability_graph(&self, limit: Option<usize>) -> ReachabilityGraph {
        let mut graph = ReachabilityGraph::new();
        let mut visited = HashMap::<Marking, usize>::new();
        let mut queue = VecDeque::new();
        let mut truncated = false;

        let initial = self.initial_marking();
        let initial_idx = graph.add_marking(initial.clone());
        visited.insert(initial, initial_idx);
        queue.push_back(initial_idx);

        while let Some(current_idx) = queue.pop_front() {
            let current_marking = graph.markings[current_idx].clone();
            let enabled = self.enabled_transitions(&current_marking);
            if enabled.is_empty() {
                graph.deadlocks.push(current_idx);
            }

            for transition in enabled {
                match self.fire_transition(&current_marking, transition) {
                    Ok(next_marking) => {
                        let target_idx = match visited.entry(next_marking.clone()) {
                            Entry::Occupied(entry) => *entry.get(),
                            Entry::Vacant(entry) => {
                                if let Some(limit) = limit {
                                    if graph.markings.len() >= limit {
                                        truncated = true;
                                        continue;
                                    }
                                }
                                let idx = graph.add_marking(next_marking.clone());
                                entry.insert(idx);
                                queue.push_back(idx);
                                idx
                            }
                        };
                        graph.add_edge(current_idx, transition, target_idx);
                    }
                    Err(_) => continue,
                }
            }
        }

        graph.truncated = truncated;
        graph
    }

    fn is_transition_enabled(&self, transition: TransitionId, marking: &Marking) -> bool {
        if transition.index() >= self.transitions_len() {
            return false;
        }
        for (place, row) in self.pre.rows().iter_enumerated() {
            let weight = row[transition.index()];
            #[cfg(feature = "inhibitor")]
            if self.is_inhibitor_arc(place, transition) {
                if marking.tokens(place) >= weight {
                    return false;
                }
                continue;
            }
            if marking.tokens(place) < weight {
                return false;
            }
        }
        true
    }

    #[cfg(feature = "inhibitor")]
    fn is_inhibitor_arc(&self, place: PlaceId, transition: TransitionId) -> bool {
        self.inhibitor
            .as_ref()
            .map(|matrix| matrix.get(place, transition))
            .unwrap_or(false)
    }

    #[cfg(not(feature = "inhibitor"))]
    #[allow(unused)]
    fn is_inhibitor_arc(&self, _place: PlaceId, _transition: TransitionId) -> bool {
        false
    }
}

impl Default for Net {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(feature = "invariants")]
impl Net {
    pub fn transition_invariants(&self) -> Vec<Vec<BigInt>> {
        let matrix = self.c_matrix();
        let rows = matrix.rows();
        let mut data = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let mut vec = Vec::with_capacity(row.len());
            for value in row.iter() {
                vec.push(BigInt::from(*value));
            }
            data.push(vec);
        }
        compute_nullspace(&data, self.transitions_len())
    }

    pub fn place_invariants(&self) -> Vec<Vec<BigInt>> {
        let matrix = self.c_matrix();
        let places = self.places_len();
        let transitions = self.transitions_len();
        let mut transposed = vec![vec![BigInt::from(0); places]; transitions];
        for (place, row) in matrix.rows().iter_enumerated() {
            for (transition_idx, value) in row.iter().enumerate() {
                transposed[transition_idx][place.index()] = BigInt::from(*value);
            }
        }
        compute_nullspace(&transposed, places)
    }
}

#[cfg(feature = "invariants")]
fn compute_nullspace(matrix: &[Vec<BigInt>], cols: usize) -> Vec<Vec<BigInt>> {
    if cols == 0 {
        return Vec::new();
    }

    let rows = matrix.len();
    if rows == 0 {
        return (0..cols)
            .map(|free_col| {
                let mut vector = vec![BigInt::from(0); cols];
                vector[free_col] = BigInt::from(1);
                vector
            })
            .collect();
    }

    let mut rref = matrix
        .iter()
        .map(|row| {
            (0..cols)
                .map(|idx| row.get(idx).cloned().unwrap_or_else(BigInt::zero).into())
                .collect::<Vec<BigRational>>()
        })
        .collect::<Vec<_>>();

    let mut pivot_cols = Vec::new();
    let mut pivot_row = 0usize;

    for col in 0..cols {
        if pivot_row >= rows {
            break;
        }
        let mut pivot = None;
        for row in pivot_row..rows {
            if !rref[row][col].is_zero() {
                pivot = Some(row);
                break;
            }
        }
        let Some(row_idx) = pivot else {
            continue;
        };

        if row_idx != pivot_row {
            rref.swap(row_idx, pivot_row);
        }

        let pivot_value = rref[pivot_row][col].clone();
        for value in rref[pivot_row].iter_mut() {
            *value /= pivot_value.clone();
        }

        for row in 0..rows {
            if row == pivot_row {
                continue;
            }
            let factor = rref[row][col].clone();
            if factor.is_zero() {
                continue;
            }
            for inner_col in col..cols {
                let adjustment = rref[pivot_row][inner_col].clone() * factor.clone();
                rref[row][inner_col] -= adjustment;
            }
        }

        pivot_cols.push(col);
        pivot_row += 1;
    }

    let mut pivot_flags = vec![false; cols];
    for &col in &pivot_cols {
        pivot_flags[col] = true;
    }

    let free_cols = (0..cols)
        .filter(|&col| !pivot_flags[col])
        .collect::<Vec<_>>();

    if free_cols.is_empty() {
        return Vec::new();
    }

    let mut basis = Vec::new();

    for &free_col in &free_cols {
        let mut vector = vec![BigRational::from_integer(BigInt::zero()); cols];
        vector[free_col] = BigRational::one();
        for (pivot_index, &pivot_col) in pivot_cols.iter().enumerate() {
            let coeff = rref[pivot_index][free_col].clone();
            if !coeff.is_zero() {
                vector[pivot_col] = -coeff;
            }
        }
        basis.push(rational_vector_to_integer(vector));
    }

    basis
        .into_iter()
        .map(normalize_integer_vector)
        .collect::<Vec<_>>()
}

#[cfg(feature = "invariants")]
fn rational_vector_to_integer(vector: Vec<BigRational>) -> Vec<BigInt> {
    let mut lcm = BigInt::one();
    for value in &vector {
        let denom = value.denom();
        if denom.is_zero() {
            continue;
        }
        lcm = lcm.lcm(denom);
    }

    vector
        .into_iter()
        .map(|value| {
            let numer = value.numer().clone();
            let denom = value.denom().clone();
            if denom.is_zero() {
                BigInt::zero()
            } else {
                let scale = &lcm / denom;
                numer * scale
            }
        })
        .collect()
}

#[cfg(feature = "invariants")]
fn normalize_integer_vector(mut vector: Vec<BigInt>) -> Vec<BigInt> {
    let mut gcd = BigInt::zero();
    for value in &vector {
        if value.is_zero() {
            continue;
        }
        let abs = value.abs();
        gcd = if gcd.is_zero() { abs } else { gcd.gcd(&abs) };
    }

    if !gcd.is_zero() && gcd != BigInt::one() {
        for value in &mut vector {
            *value /= gcd.clone();
        }
    }

    vector
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::structure::PlaceType;
    #[test]
    fn add_place_and_transition_updates_incidence() {
        let mut net = Net::empty();
        let p = net.add_place(Place::new("p", 1, 5, PlaceType::BasicBlock, String::new()));
        let t = net.add_transition(Transition::new("t"));

        net.set_input_weight(p, t, 1);
        net.set_output_weight(p, t, 1);

        assert_eq!(net.places_len(), 1);
        assert_eq!(net.transitions_len(), 1);
        assert_eq!(*net.pre.get(p, t), 1);
        assert_eq!(*net.post.get(p, t), 1);
    }

    #[test]
    fn reachability_graph_builds_states() {
        let mut net = Net::empty();
        let p0 = net.add_place(Place::new("p0", 1, 1, PlaceType::BasicBlock, String::new()));
        let p1 = net.add_place(Place::new("p1", 0, 1, PlaceType::BasicBlock, String::new()));
        let t0 = net.add_transition(Transition::new("t0"));

        net.set_input_weight(p0, t0, 1);
        net.set_output_weight(p1, t0, 1);

        let graph = net.reachability_graph(None);
        assert_eq!(graph.markings.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.deadlocks.len(), 1);
        assert!(!graph.truncated);
    }

    #[cfg(feature = "invariants")]
    #[test]
    fn invariants_simple_cycle() {
        let mut net = Net::empty();
        let p0 = net.add_place(Place::new(
            "p0",
            1,
            10,
            PlaceType::BasicBlock,
            String::new(),
        ));
        let p1 = net.add_place(Place::new(
            "p1",
            0,
            10,
            PlaceType::BasicBlock,
            String::new(),
        ));
        let t0 = net.add_transition(Transition::new("t0"));
        let t1 = net.add_transition(Transition::new("t1"));

        net.set_input_weight(p0, t0, 1);
        net.set_output_weight(p1, t0, 1);
        net.set_input_weight(p1, t1, 1);
        net.set_output_weight(p0, t1, 1);

        let p_invariants = net.place_invariants();
        let t_invariants = net.transition_invariants();

        assert!(p_invariants.iter().any(|vec| {
            vec.iter().map(|value| value.abs()).collect::<Vec<_>>()
                == vec![BigInt::from(1), BigInt::from(1)]
        }));
        assert!(t_invariants.iter().any(|vec| {
            vec.iter().map(|value| value.abs()).collect::<Vec<_>>()
                == vec![BigInt::from(1), BigInt::from(1)]
        }));
    }
}
