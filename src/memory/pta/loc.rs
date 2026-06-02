use super::context::Context;
use super::intern::Interner;

/// Projection element, decoupled from rustc `PlaceElem` so the core stays
/// rustc-free. Array/slice indices are merged into a single `Index` (sound
/// over-approximation); constant indices may be modeled later for precision.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ProjElem {
    Field(u32),
    Deref,
    Index,
}

/// Interned id for a sequence of `ProjElem` (an access path suffix).
pub type FieldPath = u32;

/// Allocation site: identifies a heap object abstractly by its creation point.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AllocSite {
    /// `InstanceId` index of the allocating function (assigned by the builder).
    pub func: u32,
    pub bb: u32,
    pub idx: u32,
}

/// The universe of abstract memory locations / pointer nodes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AbstractLoc {
    Var {
        ctx: Context,
        func: u32,
        base: u32,
        path: FieldPath,
    },
    Heap {
        ctx: Context,
        site: AllocSite,
        path: FieldPath,
    },
    Global {
        def_index: u64,
        path: FieldPath,
    },
}

/// Dense id for an interned `AbstractLoc`.
pub type LocId = u32;

/// Context-insensitive identity of an abstract location: the same memory
/// location across all calling contexts. Used to collapse k-CFA results for
/// context-insensitive queries soundly (two pointers alias if they reach the
/// same location under *any* pair of contexts).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CiKey {
    Var { func: u32, base: u32, path: FieldPath },
    Heap { site: AllocSite, path: FieldPath },
    Global { def_index: u64, path: FieldPath },
}

/// Owns the interners for field paths and abstract locations.
#[derive(Default)]
pub struct LocArena {
    paths: Interner<Vec<ProjElem>>,
    locs: Interner<AbstractLoc>,
}

impl LocArena {
    pub fn empty_path(&mut self) -> FieldPath {
        self.paths.intern(Vec::new())
    }

    pub fn extend_path(&mut self, base: FieldPath, elem: ProjElem) -> FieldPath {
        let mut v = self.paths.get(base).clone();
        v.push(elem);
        self.paths.intern(v)
    }

    pub fn path(&self, id: FieldPath) -> &[ProjElem] {
        self.paths.get(id)
    }

    /// Id of the already-interned empty path, if any path was interned.
    pub fn empty_path_id(&self) -> Option<FieldPath> {
        self.paths.get_id(&Vec::new())
    }

    pub fn var(&mut self, func: u32, base: u32, path: FieldPath) -> LocId {
        self.locs.intern(AbstractLoc::Var {
            ctx: Context::empty(),
            func,
            base,
            path,
        })
    }

    pub fn var_ctx(&mut self, ctx: Context, func: u32, base: u32, path: FieldPath) -> LocId {
        self.locs.intern(AbstractLoc::Var {
            ctx,
            func,
            base,
            path,
        })
    }

    pub fn heap(&mut self, site: AllocSite, path: FieldPath) -> LocId {
        self.locs.intern(AbstractLoc::Heap {
            ctx: Context::empty(),
            site,
            path,
        })
    }

    pub fn global(&mut self, def_index: u64, path: FieldPath) -> LocId {
        self.locs.intern(AbstractLoc::Global { def_index, path })
    }

    pub fn loc(&self, id: LocId) -> &AbstractLoc {
        self.locs.get(id)
    }

    pub fn loc_count(&self) -> usize {
        self.locs.len()
    }

    /// Iterate `(LocId, &AbstractLoc)` for every interned location.
    pub fn iter_locs(&self) -> impl Iterator<Item = (LocId, &AbstractLoc)> {
        self.locs.iter()
    }

    /// Context-insensitive identity of a location id.
    pub fn ci_key(&self, id: LocId) -> CiKey {
        match self.locs.get(id) {
            AbstractLoc::Var { func, base, path, .. } => CiKey::Var {
                func: *func,
                base: *base,
                path: *path,
            },
            AbstractLoc::Heap { site, path, .. } => CiKey::Heap {
                site: *site,
                path: *path,
            },
            AbstractLoc::Global { def_index, path } => CiKey::Global {
                def_index: *def_index,
                path: *path,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_path_intern_and_extend() {
        let mut arena = LocArena::default();
        let empty = arena.empty_path();
        let f0 = arena.extend_path(empty, ProjElem::Field(0));
        let f0_deref = arena.extend_path(f0, ProjElem::Deref);
        let empty2 = arena.empty_path();
        let f0b = arena.extend_path(empty2, ProjElem::Field(0));
        assert_eq!(f0, f0b);
        assert_ne!(f0, f0_deref);
        assert_eq!(arena.path(f0).len(), 1);
        assert_eq!(arena.path(f0_deref).len(), 2);
    }

    #[test]
    fn loc_intern_dedup() {
        let mut arena = LocArena::default();
        let p = arena.empty_path();
        let v1 = arena.var(7, 1u32, p);
        let v2 = arena.var(7, 1u32, p);
        let h = arena.heap(AllocSite { func: 7, bb: 0, idx: 0 }, p);
        assert_eq!(v1, v2);
        assert_ne!(v1, h);
        assert_eq!(arena.loc_count(), 2);
    }
}
