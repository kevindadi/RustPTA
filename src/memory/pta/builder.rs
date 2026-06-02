//! MIR → constraint translation.
//!
//! Places are resolved field-sensitively by a [`PlaceWalk`]: Field/Index
//! projections become `Offset` constraints that append to an object's access
//! path within the *same* object, while `Deref` follows a pointer via a `Load`
//! and is NEVER baked into a path. The local slot `Var{func,base,path:[]}` is
//! the addressable storage; heap objects come from call models, not per-place
//! seeding.

extern crate rustc_abi;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_middle;

use rustc_hir::def_id::DefId;
use smallvec::SmallVec;

use rustc_middle::mir::{
    AggregateKind, Body, LocalKind, Operand, Place, PlaceElem, ProjectionElem, Rvalue, StatementKind,
    TerminatorKind,
};
use rustc_middle::ty::{self, GenericArgsRef, Instance, TyCtxt, TypingEnv};
use rustc_span::Spanned;

use crate::memory::ownership;
use super::constraint::{Constraint, ConstraintSet};
use super::context::Context;
use super::loc::{AllocSite, FieldPath, LocArena, LocId, ProjElem};
use super::model::{CallNodes, ModelRegistry};
use super::typeutil::leaf_field_paths;

/// A call site recorded during constraint building, to be resolved by the
/// driver (which owns the cross-function `FuncMap`). For analyzable callees the
/// driver emits interprocedural binding; otherwise the conservative model.
pub struct PendingCall<'tcx> {
    /// Monomorphized callee, if the call is a direct `FnDef`; `None` for
    /// indirect calls (fn pointers / dynamic dispatch).
    pub callee: Option<(DefId, GenericArgsRef<'tcx>)>,
    /// Basic block of the call terminator in the caller; combined with the
    /// caller's `func` id it forms the `CallSite` for context extension.
    pub bb: u32,
    pub dest: LocId,
    pub args: SmallVec<[Option<LocId>; 4]>,
    pub fresh_heap: LocId,
}

/// Store an aggregate `value` into heap object `heap`, field-wise, and point
/// `dest` (the smart-pointer local) at `heap`. `leaf_paths` are the boxed
/// type's leaf field paths from `typeutil::leaf_field_paths`. The empty path
/// (a non-aggregate boxed value) reduces to `Copy{heap, value}`. Reused for
/// `Box`/`Arc`/`Rc::new`. rustc-free so it is unit-tested directly.
pub(crate) fn emit_boxed_value(
    arena: &mut LocArena,
    out: &mut ConstraintSet,
    dest: LocId,
    heap: LocId,
    value: LocId,
    leaf_paths: &[FieldPath],
) {
    out.add(Constraint::AddressOf { dst: dest, obj: heap });
    for &p in leaf_paths {
        if let (Some(hp), Some(vp)) = (arena.project(heap, p), arena.project(value, p)) {
            out.add(Constraint::Copy { dst: hp, src: vp });
        }
    }
}

/// rustc-free mirror of a MIR projection element used by the place walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjKind {
    Field(u32),
    Deref,
    Index,
}

/// Resolves MIR places into field-sensitive constraint nodes.
pub struct PlaceWalk<'a> {
    arena: &'a mut LocArena,
    cs: &'a mut ConstraintSet,
    func: u32,
    ctx: Context,
    /// Shared monotonic temp counter owned by the builder, so every fresh temp
    /// in a function body is distinct across statements (no cross-statement
    /// node collisions / spurious aliasing).
    next_temp: &'a mut u32,
}

impl<'a> PlaceWalk<'a> {
    pub fn new(
        arena: &'a mut LocArena,
        cs: &'a mut ConstraintSet,
        func: u32,
        next_temp: &'a mut u32,
    ) -> Self {
        Self {
            arena,
            cs,
            func,
            ctx: Context::empty(),
            next_temp,
        }
    }

    pub fn with_ctx(
        arena: &'a mut LocArena,
        cs: &'a mut ConstraintSet,
        func: u32,
        ctx: Context,
        next_temp: &'a mut u32,
    ) -> Self {
        Self {
            arena,
            cs,
            func,
            ctx,
            next_temp,
        }
    }

    fn empty(&mut self) -> FieldPath {
        self.arena.empty_path()
    }

    fn slot(&mut self, base: u32, path: FieldPath) -> LocId {
        self.arena.var_ctx(self.ctx.clone(), self.func, base, path)
    }

    fn fresh(&mut self) -> LocId {
        let id = *self.next_temp;
        *self.next_temp += 1;
        let empty = self.empty();
        self.slot(id, empty)
    }

    fn intern_suffix(&mut self, elems: &[ProjKind]) -> FieldPath {
        let mut p = self.empty();
        for e in elems {
            let pe = match e {
                ProjKind::Field(f) => ProjElem::Field(*f),
                ProjKind::Index => ProjElem::Index,
                ProjKind::Deref => unreachable!("deref is not a path elem"),
            };
            p = self.arena.extend_path(p, pe);
        }
        p
    }

    /// Node whose points-to set is the set of lvalue locations `place` denotes
    /// (i.e. what `&place` points to).
    pub fn place_addr(&mut self, local: u32, proj: &[ProjKind]) -> LocId {
        let empty = self.empty();
        let slot = self.slot(local, empty);
        let cur0 = self.fresh();
        self.cs.add(Constraint::AddressOf { dst: cur0, obj: slot }); // pts(cur0) = { V_local }
        let mut cur = cur0;
        let mut pending: Vec<ProjKind> = Vec::new();
        for e in proj {
            match e {
                ProjKind::Field(_) | ProjKind::Index => pending.push(*e),
                ProjKind::Deref => {
                    cur = self.apply_pending(cur, &mut pending); // field offset first
                    let next = self.fresh();
                    self.cs.add(Constraint::Load { dst: next, src: cur }); // *cur
                    cur = next;
                }
            }
        }
        self.apply_pending(cur, &mut pending)
    }

    fn apply_pending(&mut self, cur: LocId, pending: &mut Vec<ProjKind>) -> LocId {
        if pending.is_empty() {
            return cur;
        }
        let suffix = self.intern_suffix(pending);
        pending.clear();
        let next = self.fresh();
        self.cs.add(Constraint::Offset { dst: next, src: cur, suffix });
        next
    }

    /// Node whose points-to set is the value held at `place`.
    pub fn place_value(&mut self, local: u32, proj: &[ProjKind]) -> LocId {
        if proj.is_empty() {
            let empty = self.empty();
            return self.slot(local, empty);
        }
        let addr = self.place_addr(local, proj);
        let v = self.fresh();
        self.cs.add(Constraint::Load { dst: v, src: addr }); // value = *addr
        v
    }
}

/// Translate a single MIR `Body` into inclusion constraints.
///
/// Call terminators are dispatched through `registry`: a matching library
/// model is applied, otherwise the call is recorded for interprocedural
/// binding by the driver (it owns the cross-function `FuncMap`); see
/// [`super::interproc`].
pub fn build_body<'a, 'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    func: u32,
    ctx: Context,
    caller: Instance<'tcx>,
    registry: &ModelRegistry,
    arena: &mut LocArena,
    constraints: &mut ConstraintSet,
) -> Vec<PendingCall<'tcx>> {
    let typing_env = TypingEnv::post_analysis(tcx, caller.def_id());
    let mut builder = ConstraintBuilder {
        tcx,
        body,
        func,
        ctx,
        caller,
        typing_env,
        arena,
        constraints,
        pending: Vec::new(),
        call_counter: 0,
        next_temp: 1_000_000,
        addr_taken: rustc_data_structures::fx::FxHashSet::default(),
    };
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for stmt in &data.statements {
            if let StatementKind::Assign(box (place, rvalue)) = &stmt.kind {
                builder.process_assignment(place, rvalue);
            }
        }
        if let Some(term) = &data.terminator {
            if let TerminatorKind::Call {
                func: callee,
                args,
                destination,
                ..
            } = &term.kind
            {
                builder.process_call(bb.as_u32(), callee, args, destination, registry);
            }
        }
    }
    builder.seed_addr_taken();
    builder.pending
}

struct ConstraintBuilder<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    func: u32,
    /// Calling context this body is being built under (k-CFA). Tags every
    /// local/place node so the same instance can be cloned per context.
    ctx: Context,
    caller: Instance<'tcx>,
    typing_env: TypingEnv<'tcx>,
    arena: &'a mut LocArena,
    constraints: &'a mut ConstraintSet,
    /// Call sites awaiting interprocedural resolution by the driver.
    pending: Vec<PendingCall<'tcx>>,
    /// Monotonic counter giving each call site a distinct fresh heap object.
    call_counter: u32,
    /// Shared monotonic counter for place-walk temp nodes, threaded into every
    /// `PlaceWalk` so temps are distinct across all statements in this body.
    /// Based at `1_000_000` to stay clear of real (small-index) MIR locals.
    next_temp: u32,
    /// Locals whose storage address is taken (`r = &local`).
    addr_taken: rustc_data_structures::fx::FxHashSet<u32>,
}

impl<'a, 'tcx> ConstraintBuilder<'a, 'tcx> {
    /// Convert a rustc MIR projection into the rustc-free [`ProjKind`] slice the
    /// place walk consumes. Index / ConstantIndex / Subslice / Downcast /
    /// OpaqueCast etc. collapse into a single `Index` (sound over-approximation).
    fn proj_kinds(proj: &[PlaceElem<'tcx>]) -> SmallVec<[ProjKind; 4]> {
        let mut out: SmallVec<[ProjKind; 4]> = SmallVec::new();
        for e in proj {
            out.push(match e {
                ProjectionElem::Field(f, _) => ProjKind::Field(f.as_u32()),
                ProjectionElem::Deref => ProjKind::Deref,
                _ => ProjKind::Index,
            });
        }
        out
    }

    /// A `PlaceWalk` borrowing this builder's arena and constraint set. The
    /// returned walk mutably borrows `*self` for its lifetime, so each call
    /// MUST be scoped in its own block and dropped before any other `self.*`
    /// access.
    fn walk(&mut self) -> PlaceWalk<'_> {
        PlaceWalk::with_ctx(
            self.arena,
            self.constraints,
            self.func,
            self.ctx.clone(),
            &mut self.next_temp,
        )
    }

    /// Value node for a Move/Copy operand; `None` for constants.
    fn operand_value(&mut self, op: &Operand<'tcx>) -> Option<LocId> {
        match op {
            Operand::Move(p) | Operand::Copy(p) => {
                let proj = Self::proj_kinds(p.projection);
                let mut w = self.walk();
                Some(w.place_value(p.local.as_u32(), &proj))
            }
            _ => None,
        }
    }

    /// Store `value` into the lvalue locations of the place `lhs_local.lhs_proj`.
    fn store_value(&mut self, lhs_proj: &[ProjKind], lhs_local: u32, value: LocId) {
        if lhs_proj.is_empty() {
            self.copy_with_aggregate_expansion(lhs_local, value);
            return;
        }
        let addr = {
            let mut w = self.walk();
            w.place_addr(lhs_local, lhs_proj)
        };
        self.constraints.add(Constraint::Store { dst: addr, src: value });
    }

    fn copy_with_aggregate_expansion(&mut self, dst_local: u32, value: LocId) {
        let empty = self.arena.empty_path();
        let dst_slot = self
            .arena
            .var_ctx(self.ctx.clone(), self.func, dst_local, empty);
        // Base copy is always sound. (Type-driven field expansion for aggregate
        // local-to-local moves is wired in the driver task; base Copy preserves
        // soundness here.)
        self.constraints.add(Constraint::Copy {
            dst: dst_slot,
            src: value,
        });
    }

    fn process_assignment(&mut self, place: &Place<'tcx>, rvalue: &Rvalue<'tcx>) {
        let lhs_proj = Self::proj_kinds(place.projection);
        let lhs_local = place.local.as_u32();
        match rvalue {
            Rvalue::Aggregate(box kind, fields) => {
                self.assign_aggregate(lhs_local, &lhs_proj, kind, fields);
            }
            Rvalue::Ref(_, _, src) | Rvalue::RawPtr(_, src) => {
                if src.projection.is_empty() {
                    match self.body.local_kind(src.local) {
                        LocalKind::Arg | LocalKind::ReturnPointer => {}
                        _ => {
                            self.addr_taken.insert(src.local.as_u32());
                        }
                    }
                }
                let src_addr = {
                    let proj = Self::proj_kinds(src.projection);
                    let mut w = self.walk();
                    w.place_addr(src.local.as_u32(), &proj)
                };
                self.store_value(&lhs_proj, lhs_local, src_addr);
            }
            Rvalue::Use(op, _)
            | Rvalue::Cast(_, op, _)
            | Rvalue::Repeat(op, _)
            | Rvalue::UnaryOp(_, op) => {
                if let Some(v) = self.operand_value(op) {
                    self.store_value(&lhs_proj, lhs_local, v);
                }
            }
            Rvalue::CopyForDeref(src) | Rvalue::Discriminant(src) => {
                let v = {
                    let proj = Self::proj_kinds(src.projection);
                    let mut w = self.walk();
                    w.place_value(src.local.as_u32(), &proj)
                };
                self.store_value(&lhs_proj, lhs_local, v);
            }
            Rvalue::BinaryOp(_, box (l, r)) => {
                for op in [l, r] {
                    if let Some(v) = self.operand_value(op) {
                        self.store_value(&lhs_proj, lhs_local, v);
                    }
                }
            }
            _ => {}
        }
    }

    fn assign_aggregate(
        &mut self,
        dst_local: u32,
        lhs_proj: &[ProjKind],
        _kind: &AggregateKind<'tcx>,
        fields: &rustc_index::IndexVec<rustc_abi::FieldIdx, Operand<'tcx>>,
    ) {
        for (i, op) in fields.iter_enumerated() {
            let Some(value) = self.operand_value(op) else {
                continue;
            };
            let mut proj: SmallVec<[ProjKind; 4]> = SmallVec::from_slice(lhs_proj);
            proj.push(ProjKind::Field(i.as_u32()));
            self.store_value(&proj, dst_local, value);
        }
    }

    fn process_call(
        &mut self,
        bb: u32,
        callee: &Operand<'tcx>,
        args: &[Spanned<Operand<'tcx>>],
        destination: &Place<'tcx>,
        registry: &ModelRegistry,
    ) {
        let dest = {
            let proj = Self::proj_kinds(destination.projection);
            if proj.is_empty() {
                let empty = self.arena.empty_path();
                self.arena
                    .var_ctx(self.ctx.clone(), self.func, destination.local.as_u32(), empty)
            } else {
                // Projected destination (`(*p).f = call()`): models and binding
                // `Copy` the return value into `dest` treating it as a *value*
                // slot. Route through a fresh temp value node stored back
                // through the lvalue address, so field/deref targets receive
                // the value soundly (`*addr ⊇ temp ⊇ return`).
                let (addr, temp) = {
                    let mut w = self.walk();
                    let addr = w.place_addr(destination.local.as_u32(), &proj);
                    let temp = w.fresh();
                    (addr, temp)
                };
                self.constraints
                    .add(Constraint::Store { dst: addr, src: temp });
                temp
            }
        };

        let mut arg_nodes: SmallVec<[Option<LocId>; 4]> = SmallVec::new();
        for a in args {
            arg_nodes.push(self.operand_value(&a.node));
        }

        // Closures are interprocedural: any closure passed by value will be invoked
        // with this environment object. Bind `closure._1 ⊇ closure_object` (field-
        // expanded by the driver) so captured upvars flow into the closure body.
        for (i, a) in args.iter().enumerate() {
            let Some(Some(clo_obj)) = arg_nodes.get(i).copied() else {
                continue;
            };
            let arg_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                self.typing_env,
                ty::EarlyBinder::bind(a.node.ty(self.body, self.tcx)),
            );
            if let ty::Closure(clo_def, clo_substs) = *arg_ty.kind() {
                self.call_counter += 1;
                let empty = self.arena.empty_path();
                let clo_heap = self.arena.heap(
                    AllocSite {
                        func: self.func,
                        bb,
                        idx: self.call_counter,
                    },
                    empty,
                );
                let clo_dest = self.arena.var_ctx(
                    self.ctx.clone(),
                    self.func,
                    u32::MAX - i as u32,
                    empty,
                );
                self.pending.push(PendingCall {
                    callee: Some((clo_def, clo_substs)),
                    bb,
                    dest: clo_dest,
                    args: {
                        let mut v: SmallVec<[Option<LocId>; 4]> = SmallVec::new();
                        v.push(Some(clo_obj));
                        v
                    },
                    fresh_heap: clo_heap,
                });
            }
        }

        self.call_counter += 1;
        let empty = self.arena.empty_path();
        let fresh_heap = self.arena.heap(
            AllocSite {
                func: self.func,
                bb,
                idx: self.call_counter,
            },
            empty,
        );

        // Monomorphize the callee type in the caller's context (mirrors the
        // call-graph construction) so generic calls resolve correctly.
        let func_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
            self.tcx,
            self.typing_env,
            ty::EarlyBinder::bind(callee.ty(self.body, self.tcx)),
        );

        if let ty::FnDef(def_id, substs) = *func_ty.kind() {
            // Box/Arc/Rc::new: store the boxed aggregate into the fresh heap
            // field-wise, and point `dest` at that heap, so dereferencing any
            // clone reaches the same shared object.
            if ownership::is_box_arc_rc_new(def_id, self.tcx) {
                if let Some(Some(value)) = arg_nodes.first().copied() {
                    let boxed_ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
                        self.tcx,
                        self.typing_env,
                        ty::EarlyBinder::bind(args[0].node.ty(self.body, self.tcx)),
                    );
                    let paths = leaf_field_paths(self.tcx, self.typing_env, boxed_ty, self.arena);
                    emit_boxed_value(
                        self.arena,
                        self.constraints,
                        dest,
                        fresh_heap,
                        value,
                        &paths,
                    );
                }
                return;
            }

            let nodes = CallNodes {
                dest,
                args: arg_nodes,
                fresh_heap,
            };
            if registry.try_specialized(self.tcx, def_id, substs, &nodes, self.constraints) {
                return;
            }
            // Analyzable-or-not is decided by the driver; record for binding.
            self.pending.push(PendingCall {
                callee: Some((def_id, substs)),
                bb,
                dest: nodes.dest,
                args: nodes.args,
                fresh_heap: nodes.fresh_heap,
            });
        } else {
            // Indirect call (fn pointer / dynamic dispatch).
            self.pending.push(PendingCall {
                callee: None,
                bb,
                dest,
                args: arg_nodes,
                fresh_heap,
            });
        }
    }

    /// Seed a fresh object for each address-taken local (`r = &local`). The
    /// local's storage slot gains a concrete object so dereferencing pointers to
    /// it yields something (restores lock-behind-stack-pointer detection without
    /// the blanket per-place seeding that over-split struct fields).
    fn seed_addr_taken(&mut self) {
        let locals: Vec<u32> = self.addr_taken.iter().copied().collect();
        let empty = self.arena.empty_path();
        for base in locals {
            if !self.should_seed_addr_taken(base) {
                continue;
            }
            let slot = self
                .arena
                .var_ctx(self.ctx.clone(), self.func, base, empty);
            let obj = self.arena.heap(
                AllocSite {
                    func: self.func,
                    bb: u32::MAX,
                    idx: base,
                },
                empty,
            );
            self.constraints
                .add(Constraint::AddressOf { dst: slot, obj });
        }
    }

    /// Stack locals whose address is taken only for smart-pointer APIs (`&arc`
    /// passed to `Arc::clone`) must not receive a fresh seed — they already
    /// point at heap via `emit_boxed_value`. Seeding them duplicates the
    /// boxed object and splits lock resources.
    fn should_seed_addr_taken(&self, base: u32) -> bool {
        use rustc_middle::mir::Local;
        let local = Local::from_u32(base);
        if local.as_usize() >= self.body.local_decls.len() {
            return false;
        }
        let ty = self.caller.instantiate_mir_and_normalize_erasing_regions(
            self.tcx,
            self.typing_env,
            ty::EarlyBinder::bind(self.body.local_decls[local].ty),
        );
        if ty.is_ref() {
            return false;
        }
        let name = format!("{:?}", ty);
        ownership::is_arc(&name)
            || ownership::is_rc(&name)
            || name.starts_with("std::boxed::Box<")
    }
}

#[cfg(test)]
mod place_tests {
    use super::*;
    use crate::memory::pta::constraint::{Constraint, ConstraintSet};
    use crate::memory::pta::loc::{LocArena, ProjElem};
    use crate::memory::pta::solver::Solver;

    fn mk<'a>(
        arena: &'a mut LocArena,
        cs: &'a mut ConstraintSet,
        next_temp: &'a mut u32,
    ) -> PlaceWalk<'a> {
        PlaceWalk::new(arena, cs, /*func*/ 0, next_temp)
    }

    #[test]
    fn ref_of_field_through_deref_is_object_field() {
        // _1 (self) = &O ; r = &(*_1).0  ⇒ pts(r) = { O·0 }.
        let mut arena = LocArena::default();
        let mut cs = ConstraintSet::default();
        let empty = arena.empty_path();
        let f0 = arena.extend_path(empty, ProjElem::Field(0));
        let o = arena.heap(
            crate::memory::pta::loc::AllocSite { func: 0, bb: 0, idx: 0 },
            empty,
        );
        let v1 = arena.var_ctx(crate::memory::pta::context::Context::empty(), 0, 1, empty);
        cs.add(Constraint::AddressOf { dst: v1, obj: o }); // self = &O

        let mut next_temp = 1_000_000u32;
        let r = {
            let mut w = mk(&mut arena, &mut cs, &mut next_temp);
            w.place_addr(1, &[ProjKind::Deref, ProjKind::Field(0)])
        };

        let o_f0 = arena.heap(
            crate::memory::pta::loc::AllocSite { func: 0, bb: 0, idx: 0 },
            f0,
        );
        let pts = Solver::new(0).solve(&cs, &mut arena);
        assert!(pts.points_to(r).contains(&o_f0));
    }

    #[test]
    fn boxed_value_stores_fields_into_heap_and_points_dest_at_it() {
        use crate::memory::pta::loc::AllocSite;
        let mut arena = LocArena::default();
        let mut cs = ConstraintSet::default();
        let empty = arena.empty_path();
        let f0 = arena.extend_path(empty, ProjElem::Field(0));

        // value = the aggregate being boxed; its field .0 holds a lock object L.
        let value = arena.var(0, 5, empty);
        let value_f0 = arena.var(0, 5, f0);
        let lock = arena.heap(AllocSite { func: 0, bb: 1, idx: 1 }, empty);
        cs.add(Constraint::AddressOf {
            dst: value_f0,
            obj: lock,
        }); // value.0 = &L

        let dest = arena.var(0, 6, empty); // the Arc local
        let heap = arena.heap(AllocSite { func: 0, bb: 2, idx: 1 }, empty); // H_arc

        // leaf paths for a 1-field aggregate: [.0]
        super::emit_boxed_value(&mut arena, &mut cs, dest, heap, value, &[f0]);

        let heap_f0 = arena.heap(AllocSite { func: 0, bb: 2, idx: 1 }, f0);
        let pts = Solver::new(0).solve(&cs, &mut arena);
        // dest points at the heap, and heap.0 carries the lock (shared content).
        assert!(pts.points_to(dest).contains(&heap));
        assert!(pts.points_to(heap_f0).contains(&lock));
    }
}
