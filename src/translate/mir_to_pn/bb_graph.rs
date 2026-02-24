//! BasicBlockGraph 与 SegState：基本块图结构与原子序分段状态

use crate::net::PlaceId;
use rustc_middle::mir::BasicBlock;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct BasicBlockGraph {
    pub start_places: HashMap<BasicBlock, PlaceId>,
    pub sequences: HashMap<BasicBlock, Vec<PlaceId>>,
}

impl BasicBlockGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, bb: BasicBlock, start: PlaceId) {
        self.start_places.insert(bb, start);
        self.sequences.insert(bb, vec![start]);
    }

    pub fn push(&mut self, bb: BasicBlock, place: PlaceId) {
        self.sequences.entry(bb).or_default().push(place);
    }

    pub fn start(&self, bb: BasicBlock) -> PlaceId {
        *self
            .start_places
            .get(&bb)
            .expect("basic block start place should exist")
    }

    pub fn last(&self, bb: BasicBlock) -> PlaceId {
        *self
            .sequences
            .get(&bb)
            .and_then(|nodes| nodes.last())
            .expect("basic block last node should exist")
    }
}

#[cfg(feature = "atomic-violation")]
#[derive(Default)]
pub(super) struct SegState {
    pub seg_index: HashMap<usize, usize>,
    pub seg_place_of: HashMap<(usize, usize), PlaceId>,
    pub seqcst_place: Option<PlaceId>,
}

#[cfg(feature = "atomic-violation")]
impl SegState {
    pub fn current_seg(&self, tid: usize) -> usize {
        *self.seg_index.get(&tid).unwrap_or(&0)
    }

    pub fn bump(&mut self, tid: usize) -> usize {
        let next = self.current_seg(tid) + 1;
        self.seg_index.insert(tid, next);
        next
    }
}
