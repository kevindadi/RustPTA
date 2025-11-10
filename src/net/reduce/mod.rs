//! Petri 网缩减算法 :循环剔除、序列合并与中介库所消除.
use std::sync::Arc;

use thiserror::Error;

use crate::net::ids::{PlaceId, TransitionId};
use crate::net::index_vec::IndexVec;
use crate::net::Net;

mod graph;
mod intermediate_place;
mod loop_removal;
mod sequence_merge;

use graph::{MaterializedNet, ReductionGraph};

pub type ReductionValidator = dyn Fn(&Net) -> Result<(), ReductionError> + Send + Sync;

#[derive(Default, Clone)]
pub struct ReductionOptions {
    /// 在每步缩减后执行的不变量校验
    pub invariant_checker: Option<Arc<ReductionValidator>>,
    /// todo: 添加属性检查器 死锁那些玩意
    pub property_checker: Option<Arc<ReductionValidator>>,
}

#[derive(Debug)]
pub struct ReductionResult {
    pub net: Net,
    pub trace: ReductionTrace,
    pub steps: Vec<ReductionStep>,
}

#[derive(Debug, Clone)]
pub struct ReductionTrace {
    pub place_mapping: IndexVec<PlaceId, Vec<PlaceId>>,
    pub transition_mapping: IndexVec<TransitionId, Vec<TransitionId>>,
}

#[derive(Debug, Clone)]
pub enum ReductionStep {
    LoopRemoved {
        removed_places: Vec<PlaceId>,
        removed_transitions: Vec<TransitionId>,
    },
    SequenceMerged {
        head_places: Vec<PlaceId>,
        tail_places: Vec<PlaceId>,
        merged_transitions: Vec<TransitionId>,
        removed_places: Vec<PlaceId>,
    },
    IntermediatePlaceEliminated {
        places: Vec<PlaceId>,
        merged_transitions: Vec<TransitionId>,
    },
}

#[derive(Debug, Error)]
pub enum ReductionError {
    #[error("validator rejected reduced net: {0}")]
    ValidationFailed(String),
}

pub struct Reducer {
    options: ReductionOptions,
}

impl Reducer {
    pub fn new(options: ReductionOptions) -> Self {
        Self { options }
    }

    pub fn reduce(&self, net: &mut Net) -> Result<ReductionResult, ReductionError> {
        let mut graph = ReductionGraph::from_net(net);
        let mut steps = Vec::new();

        let loop_steps = graph.remove_simple_loops();
        if !loop_steps.is_empty() {
            steps.extend(loop_steps);
            self.validate(&graph)?;
        }

        let sequence_steps = graph.merge_linear_sequences();
        if !sequence_steps.is_empty() {
            steps.extend(sequence_steps);
            self.validate(&graph)?;
        }

        let intermediate_steps = graph.eliminate_intermediate_places();
        if !intermediate_steps.is_empty() {
            steps.extend(intermediate_steps);
            self.validate(&graph)?;
        }

        let MaterializedNet {
            net: reduced_net,
            trace,
        } = graph.materialize();
        *net = reduced_net.clone();

        Ok(ReductionResult {
            net: reduced_net,
            trace,
            steps,
        })
    }

    fn validate(&self, graph: &ReductionGraph) -> Result<(), ReductionError> {
        if self.options.invariant_checker.is_none() && self.options.property_checker.is_none() {
            return Ok(());
        }

        let materialized = graph.materialize();
        if let Some(checker) = &self.options.invariant_checker {
            checker(&materialized.net)
                .map_err(|err| ReductionError::ValidationFailed(err.to_string()))?;
        }
        if let Some(checker) = &self.options.property_checker {
            checker(&materialized.net)
                .map_err(|err| ReductionError::ValidationFailed(err.to_string()))?;
        }
        Ok(())
    }
}

pub fn reduce_in_place(
    net: &mut Net,
    options: ReductionOptions,
) -> Result<ReductionResult, ReductionError> {
    Reducer::new(options).reduce(net)
}
