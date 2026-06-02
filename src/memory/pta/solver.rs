use std::collections::VecDeque;

use rustc_data_structures::fx::{FxHashMap, FxHashSet};

use super::constraint::{Constraint, ConstraintSet};
use super::loc::LocId;

/// Solved points-to relation: `LocId` -> set of pointee `LocId`s.
#[derive(Default)]
pub struct PointsTo {
    map: FxHashMap<LocId, FxHashSet<LocId>>,
}

impl PointsTo {
    pub fn points_to(&self, n: LocId) -> &FxHashSet<LocId> {
        static EMPTY: once_cell::sync::Lazy<FxHashSet<LocId>> =
            once_cell::sync::Lazy::new(FxHashSet::default);
        self.map.get(&n).unwrap_or(&EMPTY)
    }

    pub fn raw(&self) -> &FxHashMap<LocId, FxHashSet<LocId>> {
        &self.map
    }

    fn insert(&mut self, n: LocId, o: LocId) -> bool {
        self.map.entry(n).or_default().insert(o)
    }
}

/// Standard Andersen inclusion-based worklist solver.
pub struct Solver {
    loc_count: usize,
}

impl Solver {
    pub fn new(loc_count: usize) -> Self {
        Self { loc_count }
    }

    pub fn solve(&self, constraints: &ConstraintSet) -> PointsTo {
        let mut pts = PointsTo::default();

        // copy edge `src -> {dst}` meaning `dst ⊇ src`.
        let mut copy_succ: FxHashMap<LocId, FxHashSet<LocId>> = FxHashMap::default();
        // `dst ⊇ *src` keyed by src.
        let mut loads: FxHashMap<LocId, FxHashSet<LocId>> = FxHashMap::default();
        // `*dst ⊇ src` keyed by dst.
        let mut stores: FxHashMap<LocId, FxHashSet<LocId>> = FxHashMap::default();

        let mut worklist: VecDeque<LocId> = VecDeque::new();
        let mut in_wl: FxHashSet<LocId> = FxHashSet::default();

        for c in constraints.iter() {
            match *c {
                Constraint::AddressOf { dst, obj } => {
                    if pts.insert(dst, obj) && in_wl.insert(dst) {
                        worklist.push_back(dst);
                    }
                }
                Constraint::Copy { dst, src } => {
                    copy_succ.entry(src).or_default().insert(dst);
                    if in_wl.insert(src) {
                        worklist.push_back(src);
                    }
                }
                Constraint::Load { dst, src } => {
                    loads.entry(src).or_default().insert(dst);
                    if in_wl.insert(src) {
                        worklist.push_back(src);
                    }
                }
                Constraint::Store { dst, src } => {
                    stores.entry(dst).or_default().insert(src);
                    if in_wl.insert(dst) {
                        worklist.push_back(dst);
                    }
                }
                // TODO(Task 3): handle field-sensitive Offset constraints.
                // Temporary no-op arm added in Task 1 only to keep the crate
                // compiling; replaced when Offset solving is implemented.
                Constraint::Offset { .. } => {}
            }
        }

        while let Some(n) = worklist.pop_front() {
            in_wl.remove(&n);
            let pointees: Vec<LocId> = pts.points_to(n).iter().copied().collect();

            // Copy edges: for each `dst ⊇ n`, propagate pts(n) into pts(dst).
            if let Some(succs) = copy_succ.get(&n) {
                let succs: Vec<LocId> = succs.iter().copied().collect();
                for dst in succs {
                    let mut changed = false;
                    for &o in &pointees {
                        changed |= pts.insert(dst, o);
                    }
                    if changed && in_wl.insert(dst) {
                        worklist.push_back(dst);
                    }
                }
            }

            // Load `dst ⊇ *n`: for each o in pts(n), add copy edge `o -> dst`.
            if let Some(dsts) = loads.get(&n) {
                let dsts: Vec<LocId> = dsts.iter().copied().collect();
                for dst in dsts {
                    for &o in &pointees {
                        if copy_succ.entry(o).or_default().insert(dst) && in_wl.insert(o) {
                            worklist.push_back(o);
                        }
                    }
                }
            }

            // Store `*n ⊇ src`: for each o in pts(n), add copy edge `src -> o`.
            if let Some(srcs) = stores.get(&n) {
                let srcs: Vec<LocId> = srcs.iter().copied().collect();
                for src in srcs {
                    for &o in &pointees {
                        if copy_succ.entry(src).or_default().insert(o) && in_wl.insert(src) {
                            worklist.push_back(src);
                        }
                    }
                }
            }
        }

        let _ = self.loc_count;
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::pta::constraint::{Constraint, ConstraintSet};

    #[test]
    fn copy_propagates_points_to() {
        let (p, q, a) = (0u32, 1u32, 2u32);
        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf { dst: p, obj: a });
        cs.add(Constraint::Copy { dst: q, src: p });
        let pts = Solver::new(3).solve(&cs);
        assert!(pts.points_to(q).contains(&a));
    }

    #[test]
    fn load_store_through_pointer() {
        let (p, r, a, b) = (0u32, 1u32, 2u32, 3u32);
        let tb = 4u32;
        let mut cs = ConstraintSet::default();
        cs.add(Constraint::AddressOf { dst: p, obj: a });
        cs.add(Constraint::AddressOf { dst: tb, obj: b });
        cs.add(Constraint::Store { dst: p, src: tb });
        cs.add(Constraint::Load { dst: r, src: p });
        let pts = Solver::new(5).solve(&cs);
        assert!(pts.points_to(a).contains(&b));
        assert!(pts.points_to(r).contains(&b));
    }
}
