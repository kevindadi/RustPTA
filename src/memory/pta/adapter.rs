//! Compatibility shim bridging the new engine to the legacy alias-query API.
//!
//! [`PtaAliasAnalysis`] reuses the existing [`AliasId`] / [`ApproximateAliasKind`]
//! types and exposes `alias` / `alias_atomic` / `points_to` backed by the new
//! [`PointerAnalysis`]. This lets Petri-net construction switch to the new
//! engine without changing call sites (migration happens in a later task).
//!
//! `AliasId::array_index` (which distinguishes `arr[0]` from `arr[1]`) is not
//! honored: the engine merges array indices, a sound over-approximation that
//! can only widen alias results relative to the legacy analysis.

extern crate rustc_middle;

use rustc_middle::ty::Instance;

use super::analysis::PointerAnalysis;
use super::loc::LocId;
use super::result::PointsToResult;
use crate::memory::pointsto::{AliasId, ApproximateAliasKind};
use crate::translate::callgraph::CallGraph;

pub struct PtaAliasAnalysis<'a, 'tcx> {
    pta: PointerAnalysis<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
    result: Option<PointsToResult>,
}

impl<'a, 'tcx> PtaAliasAnalysis<'a, 'tcx> {
    pub fn new(tcx: rustc_middle::ty::TyCtxt<'tcx>, callgraph: &'a CallGraph<'tcx>) -> Self {
        Self {
            pta: PointerAnalysis::new(tcx),
            callgraph,
            result: None,
        }
    }

    /// Build constraints for all call-graph instances and solve. Idempotent-ish:
    /// re-solving simply recomputes from the accumulated constraints.
    pub fn build(&mut self) {
        let roots: Vec<Instance<'tcx>> = self
            .callgraph
            .graph
            .node_indices()
            .filter_map(|idx| self.callgraph.index_to_instance(idx))
            .map(|node| *node.instance())
            .collect();
        self.pta.build_reachable(roots);
        self.result = Some(self.pta.solve());
    }

    fn loc(&mut self, aid: AliasId) -> Option<LocId> {
        let instance = *self.callgraph.index_to_instance(aid.instance_id)?.instance();
        Some(self.pta.node_of(instance, aid.local.as_u32()))
    }

    /// May `aid1` and `aid2` alias?
    pub fn alias(&mut self, aid1: AliasId, aid2: AliasId) -> ApproximateAliasKind {
        if aid1.instance_id == aid2.instance_id && aid1.local == aid2.local {
            return ApproximateAliasKind::Probably;
        }
        let la = self.loc(aid1);
        let lb = self.loc(aid2);
        match (la, lb, &self.result) {
            (Some(la), Some(lb), Some(result)) => {
                if result.may_alias(la, lb) {
                    ApproximateAliasKind::Probably
                } else {
                    ApproximateAliasKind::Unlikely
                }
            }
            _ => ApproximateAliasKind::Unknown,
        }
    }

    /// Atomic-context alias query. Same semantics as [`Self::alias`] under the
    /// unified engine (the legacy split was an artifact of the heuristic layer).
    pub fn alias_atomic(&mut self, aid1: AliasId, aid2: AliasId) -> ApproximateAliasKind {
        self.alias(aid1, aid2)
    }

    /// May `pointer` point to `pointee`?
    pub fn points_to(&mut self, pointer: AliasId, pointee: AliasId) -> ApproximateAliasKind {
        let lp = self.loc(pointer);
        let lt = self.loc(pointee);
        match (lp, lt, &self.result) {
            (Some(lp), Some(lt), Some(result)) => {
                if result.points_to(lp).contains(&lt) {
                    ApproximateAliasKind::Probably
                } else {
                    ApproximateAliasKind::Unlikely
                }
            }
            _ => ApproximateAliasKind::Unknown,
        }
    }
}
