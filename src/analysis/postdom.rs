#![allow(dead_code)]
extern crate rustc_data_structures;
extern crate rustc_index;
extern crate rustc_middle;

use rustc_data_structures::graph::{
    ControlFlowGraph, DirectedGraph, WithNumNodes, WithPredecessors, WithSuccessors,
};

use rustc_index::{Idx, IndexVec};
use rustc_middle::mir::{BasicBlock, BasicBlocks, Location, TerminatorKind};
use std::borrow::Borrow;

pub(crate) fn post_dominates(
    this: Location,
    other: Location,
    post_dominators: &PostDominators<BasicBlock>,
) -> bool {
    if this.block == other.block {
        other.statement_index <= this.statement_index
    } else {
        post_dominators.is_post_dominated_by(other.block, this.block)
    }
}

pub trait WithEndNodes: DirectedGraph {
    fn end_nodes(&self) -> Vec<Self::Node>;
}

impl<'graph, G: WithEndNodes> WithEndNodes for &'graph G {
    fn end_nodes(&self) -> Vec<Self::Node> {
        (**self).end_nodes()
    }
}

impl<'tcx> WithEndNodes for BasicBlocks<'tcx> {
    #[inline]
    fn end_nodes(&self) -> Vec<Self::Node> {
        self.iter_enumerated().filter_map(|bb, bb_data| {
            if self.successors(bb).count == 0 {
                if bb_data.terminator().kind() == TerminatorKind::Return {
                    Some(bb)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}
