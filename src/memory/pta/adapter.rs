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
use super::result::PointsToResult;
use crate::memory::pointsto::{AliasId, ApproximateAliasKind};
use crate::translate::callgraph::CallGraph;

pub struct PtaAliasAnalysis<'a, 'tcx> {
    pta: PointerAnalysis<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
    result: Option<PointsToResult>,
}

impl<'a, 'tcx> PtaAliasAnalysis<'a, 'tcx> {
    /// Context-insensitive (`k = 0`) shim. Prefer [`Self::with_k`].
    pub fn new(tcx: rustc_middle::ty::TyCtxt<'tcx>, callgraph: &'a CallGraph<'tcx>) -> Self {
        Self::with_k(tcx, callgraph, 0)
    }

    /// Shim using call-site sensitivity depth `k` (k-CFA).
    pub fn with_k(
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        callgraph: &'a CallGraph<'tcx>,
        k: usize,
    ) -> Self {
        Self {
            pta: PointerAnalysis::with_k(tcx, k),
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

    fn instance_of(&self, aid: AliasId) -> Option<Instance<'tcx>> {
        Some(*self.callgraph.index_to_instance(aid.instance_id)?.instance())
    }

    /// May `aid1` and `aid2` alias? Uses context-collapsed points-to sets so the
    /// result is sound regardless of the configured k-CFA depth.
    pub fn alias(&mut self, aid1: AliasId, aid2: AliasId) -> ApproximateAliasKind {
        if aid1.instance_id == aid2.instance_id && aid1.local == aid2.local {
            return ApproximateAliasKind::Probably;
        }
        let ia = self.instance_of(aid1);
        let ib = self.instance_of(aid2);
        match (ia, ib, &self.result) {
            (Some(ia), Some(ib), Some(result)) => {
                if self.pta.collapsed_may_alias(
                    result,
                    ia,
                    aid1.local.as_u32(),
                    ib,
                    aid2.local.as_u32(),
                ) {
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

    /// Human-readable dump of the solved points-to relation (for differential
    /// comparison against the legacy engine). Call after [`Self::build`].
    pub fn format_report(&self) -> String {
        self.pta.format_report()
    }

    /// May `pointer` point to `pointee`?
    pub fn points_to(&mut self, pointer: AliasId, pointee: AliasId) -> ApproximateAliasKind {
        let ip = self.instance_of(pointer);
        let it = self.instance_of(pointee);
        match (ip, it, &self.result) {
            (Some(ip), Some(it), Some(result)) => {
                if self.pta.collapsed_points_to_local(
                    result,
                    ip,
                    pointer.local.as_u32(),
                    it,
                    pointee.local.as_u32(),
                ) {
                    ApproximateAliasKind::Probably
                } else {
                    ApproximateAliasKind::Unlikely
                }
            }
            _ => ApproximateAliasKind::Unknown,
        }
    }
}
