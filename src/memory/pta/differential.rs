//! Differential regression helpers for the new pointer-analysis engine.
//!
//! The new engine is designed to be **sound but possibly less precise** than
//! the legacy `AliasAnalysis` on some queries (e.g. array-index merging widens
//! alias sets). For migration we require that every *definite* alias reported
//! by the legacy analysis remains reachable under the new engine once node ids
//! are mapped — equivalently, the new may-alias relation should be a superset
//! of the legacy one on comparable nodes.
//!
//! End-to-end comparison is triggered from `callback.rs` (parallel
//! `points_to_report_pta.txt` export). The pure functions here support unit
//! tests and offline diff tooling without pulling in rustc.

use rustc_data_structures::fx::FxHashSet;
use smallvec::SmallVec;

use super::constraint::{Constraint, ConstraintSet};
use super::loc::{LocArena, LocId};
use super::model::{CallNodes, ModelRegistry};
use super::result::PointsToResult;
use super::solver::Solver;

/// Collect unordered may-alias pairs among `nodes` according to `result`.
pub fn alias_pairs(result: &PointsToResult, nodes: &[LocId]) -> FxHashSet<(LocId, LocId)> {
    let mut pairs = FxHashSet::default();
    for (i, &a) in nodes.iter().enumerate() {
        for &b in &nodes[i + 1..] {
            if result.may_alias(a, b) {
                let key = if a <= b { (a, b) } else { (b, a) };
                pairs.insert(key);
            }
        }
    }
    pairs
}

/// True when every may-alias pair in `baseline` also appears in `candidate`.
pub fn alias_relation_is_superset(
    baseline: &PointsToResult,
    candidate: &PointsToResult,
    nodes: &[LocId],
) -> bool {
    let base = alias_pairs(baseline, nodes);
    let cand = alias_pairs(candidate, nodes);
    base.is_subset(&cand)
}

/// True when `candidate`'s points-to sets are supersets of `baseline`'s for
/// every node in `nodes`.
pub fn points_to_is_superset(
    baseline: &PointsToResult,
    candidate: &PointsToResult,
    nodes: &[LocId],
) -> bool {
    nodes
        .iter()
        .all(|&n| baseline.points_to(n).is_subset(candidate.points_to(n)))
}

fn solve(constraints: &ConstraintSet, loc_count: usize) -> PointsToResult {
    PointsToResult::new(Solver::new(loc_count).solve(constraints))
}

#[cfg(test)]
mod tests {
    use super::super::constraint::{Constraint, ConstraintSet};
    use super::super::solver::Solver;
    use super::*;

    #[test]
    fn alias_superset_holds_when_candidate_adds_pairs() {
        let nodes = [0u32, 1u32, 2u32, 3u32];
        let mut base_cs = ConstraintSet::default();
        base_cs.add(Constraint::AddressOf { dst: 0, obj: 2 });
        base_cs.add(Constraint::AddressOf { dst: 1, obj: 3 });
        let baseline = solve(&base_cs, 4);

        let mut cand_cs = base_cs.clone();
        cand_cs.add(Constraint::AddressOf { dst: 0, obj: 3 });
        let candidate = solve(&cand_cs, 4);

        assert!(alias_relation_is_superset(&baseline, &candidate, &nodes));
        assert!(!alias_relation_is_superset(&candidate, &baseline, &nodes));
    }

    #[test]
    fn points_to_superset_detects_missing_pointee() {
        let nodes = [0u32, 1u32];
        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf { dst: 0, obj: 1 });
        let baseline = solve(&cs, 2);

        let mut wider = ConstraintSet::default();
        wider.add(Constraint::AddressOf { dst: 0, obj: 1 });
        wider.add(Constraint::Copy { dst: 0, src: 0 });
        let candidate = solve(&wider, 2);

        assert!(points_to_is_superset(&baseline, &candidate, &nodes));
    }

    #[test]
    fn unknown_model_widens_relative_to_empty_baseline() {
        let dest = 0u32;
        let arg_ptr = 1u32;
        let heap = 2u32;
        let pointee = 3u32;

        let mut baseline_cs = ConstraintSet::default();
        baseline_cs.add(Constraint::AddressOf {
            dst: arg_ptr,
            obj: pointee,
        });
        let baseline = solve(&baseline_cs, 4);

        let mut cand_cs = baseline_cs.clone();
        let registry = ModelRegistry::builtin();
        let nodes = CallNodes {
            dest,
            args: SmallVec::from_vec(vec![Some(arg_ptr)]),
            fresh_heap: heap,
        };
        registry.apply_unknown(&nodes, &mut cand_cs);
        let candidate = solve(&cand_cs, 4);

        let watch = [dest, arg_ptr, heap, pointee];
        assert!(points_to_is_superset(&baseline, &candidate, &watch));
        assert!(alias_relation_is_superset(&baseline, &candidate, &watch));
        assert!(candidate.points_to(dest).contains(&heap));
    }

    #[test]
    fn merged_array_index_path_is_sound_over_approximation() {
        // Legacy distinguishes arr[0] vs arr[1]; the new engine merges indices
        // into one `Index` path. Two pointers to different constant indices
        // therefore alias in the new engine if they share the container.
        let mut arena = LocArena::default();
        let empty = arena.empty_path();
        let idx = arena.extend_path(empty, super::super::loc::ProjElem::Index);
        let arr = arena.var(0, 1, empty);
        let arr_idx = arena.var(0, 1, idx);
        let p0 = arena.var(0, 2, empty);
        let p1 = arena.var(0, 3, empty);
        let obj = arena.heap(
            super::super::loc::AllocSite {
                func: 0,
                bb: 0,
                idx: 0,
            },
            empty,
        );

        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf {
            dst: p0,
            obj: arr_idx,
        });
        cs.add(Constraint::AddressOf {
            dst: p1,
            obj: arr_idx,
        });
        cs.add(Constraint::AddressOf { dst: arr, obj: obj });
        let result = solve(&cs, arena.loc_count());

        assert!(result.may_alias(p0, p1));
        assert!(result.points_to(p0).contains(&arr_idx));
    }

    #[test]
    fn interproc_copy_preserves_param_to_return_flow() {
        let param = 1u32;
        let ret = 0u32;
        let arg = 4u32;
        let dest = 5u32;
        let heap = 6u32;

        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf {
            dst: arg,
            obj: heap,
        });
        for edge in super::super::interproc::bind_call_edges(
            &[param],
            ret,
            &SmallVec::<[Option<LocId>; 4]>::from_vec(vec![Some(arg)]),
            dest,
        ) {
            cs.add(edge);
        }
        // Callee body: `return param` (simulated).
        cs.add(Constraint::Copy {
            dst: ret,
            src: param,
        });
        let result = solve(&cs, 7);

        assert!(result.points_to(dest).contains(&heap));
        assert!(result.points_to(ret).contains(&heap));
    }

    #[test]
    fn solver_copy_chain_matches_manual_expectation() {
        let (a, b, c, obj) = (0u32, 1u32, 2u32, 3u32);
        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf { dst: a, obj });
        cs.add(Constraint::Copy { dst: b, src: a });
        cs.add(Constraint::Copy { dst: c, src: b });
        let pts = Solver::new(4).solve(&cs);
        assert!(pts.points_to(c).contains(&obj));
    }
}
