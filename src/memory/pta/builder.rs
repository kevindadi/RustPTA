//! MIR → constraint translation.
//!
//! The pure assignment-classification logic (`assignment_edges`) is rustc-free
//! and unit tested. The MIR walk faithfully ports the proven place/operand/
//! rvalue handling from `crate::memory::pointsto`, emitting the new `Constraint`
//! IR over interned `LocId`s. Call terminators are handled in a later task.

extern crate rustc_hir;
extern crate rustc_middle;

use rustc_hir::def_id::DefId;
use smallvec::SmallVec;

use rustc_middle::mir::{
    AggregateKind, Body, Local, Operand, Place, PlaceElem, ProjectionElem, Rvalue, StatementKind,
    TerminatorKind,
};
use rustc_middle::ty::{self, GenericArgsRef, Instance, TyCtxt, TypingEnv};
use rustc_span::Spanned;

use super::constraint::{Constraint, ConstraintSet};
use super::context::Context;
use super::loc::{AllocSite, FieldPath, LocArena, LocId, ProjElem};
use super::model::{CallNodes, ModelRegistry};

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

/// Whether the left-hand side of an assignment is a direct place (`x`) or an
/// indirect store through a pointer (`*x`, with the leading `Deref` stripped).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Direct,
    Indirect,
}

/// The pointer-relevant shape of a right-hand side operand/rvalue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `&place` — address-of.
    Ref,
    /// A plain place read.
    Direct,
    /// A read through a pointer (`*place`).
    Indirect,
}

/// The inclusion-constraint shape an assignment expands into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    AddressOf,
    Copy,
    Load,
    Store,
}

/// Pure mapping from `(lhs form, rhs kind)` to the constraint edges to emit.
///
/// Mirrors the proven `process_assignment` match in `crate::memory::pointsto`.
pub fn assignment_edges(lhs: Form, rhs: Kind) -> SmallVec<[EdgeKind; 2]> {
    let mut out = SmallVec::new();
    match (lhs, rhs) {
        (Form::Direct, Kind::Ref) => out.push(EdgeKind::AddressOf),
        (Form::Direct, Kind::Direct) => out.push(EdgeKind::Copy),
        (Form::Direct, Kind::Indirect) => out.push(EdgeKind::Load),
        (Form::Indirect, Kind::Direct) => out.push(EdgeKind::Store),
        (Form::Indirect, Kind::Ref) => {
            // `*x = &y`: store the address of y through pointer x.
            out.push(EdgeKind::Store);
            out.push(EdgeKind::AddressOf);
        }
        (Form::Indirect, Kind::Indirect) => out.push(EdgeKind::Load),
    }
    out
}

/// Translate a single MIR `Body` into inclusion constraints.
///
/// Call terminators are dispatched through `registry`: a matching library
/// model is applied, otherwise the conservative unknown-callee model is used.
/// Interprocedural binding for analyzable callees is performed by the driver
/// (it owns the cross-function `FuncMap`); see [`super::interproc`].
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
        vars: Vec::new(),
        pending: Vec::new(),
        call_counter: 0,
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
    builder.seed_allocs();
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
    /// Distinct place nodes created, used to seed per-place allocation objects.
    vars: Vec<(LocId, u32, FieldPath)>,
    /// Call sites awaiting interprocedural resolution by the driver.
    pending: Vec<PendingCall<'tcx>>,
    /// Monotonic counter giving each call site a distinct fresh heap object.
    call_counter: u32,
}

impl<'a, 'tcx> ConstraintBuilder<'a, 'tcx> {
    fn proj_elem(e: &PlaceElem<'tcx>) -> ProjElem {
        match e {
            ProjectionElem::Field(f, _) => ProjElem::Field(f.as_u32()),
            ProjectionElem::Deref => ProjElem::Deref,
            // Index / ConstantIndex / Subslice / Downcast / OpaqueCast etc. are
            // merged into a single `Index` element (sound over-approximation).
            _ => ProjElem::Index,
        }
    }

    fn intern_path(&mut self, proj: &[PlaceElem<'tcx>]) -> FieldPath {
        let mut p = self.arena.empty_path();
        for e in proj {
            let pe = Self::proj_elem(e);
            p = self.arena.extend_path(p, pe);
        }
        p
    }

    fn place_node(&mut self, local: Local, proj: &[PlaceElem<'tcx>]) -> LocId {
        let path = self.intern_path(proj);
        let id = self
            .arena
            .var_ctx(self.ctx.clone(), self.func, local.as_u32(), path);
        self.vars.push((id, local.as_u32(), path));
        id
    }

    /// Returns `(node, form)` for the left-hand place. A leading `Deref` makes
    /// it an indirect store target with the `Deref` stripped.
    fn process_place(&mut self, place: &Place<'tcx>) -> (LocId, Form) {
        let pr = place.as_ref();
        match pr.projection {
            [ProjectionElem::Deref, remain @ ..] => {
                (self.place_node(pr.local, remain), Form::Indirect)
            }
            _ => (self.place_node(pr.local, pr.projection), Form::Direct),
        }
    }

    fn process_operand(&mut self, operand: &Operand<'tcx>) -> Option<(LocId, Kind)> {
        match operand {
            Operand::Move(place) | Operand::Copy(place) => {
                let pr = place.as_ref();
                Some((self.place_node(pr.local, pr.projection), Kind::Direct))
            }
            // Constants (incl. statics) and runtime-check operands carry no
            // pointer info in this task; constant/static modeling is deferred.
            _ => None,
        }
    }

    fn process_rvalue(&mut self, rvalue: &Rvalue<'tcx>) -> SmallVec<[(LocId, Kind); 2]> {
        let mut out: SmallVec<[(LocId, Kind); 2]> = SmallVec::new();
        match rvalue {
            Rvalue::Use(op, _) | Rvalue::Repeat(op, _) | Rvalue::Cast(_, op, _)
            | Rvalue::UnaryOp(_, op) => {
                if let Some(x) = self.process_operand(op) {
                    out.push(x);
                }
            }
            Rvalue::RawPtr(_, place)
            | Rvalue::Discriminant(place)
            | Rvalue::CopyForDeref(place) => {
                let pr = place.as_ref();
                match pr.projection {
                    [ProjectionElem::Deref, remain @ ..] => {
                        out.push((self.place_node(pr.local, remain), Kind::Direct));
                    }
                    _ => {
                        out.push((self.place_node(pr.local, pr.projection), Kind::Ref));
                    }
                }
            }
            Rvalue::Ref(_, _, place) => {
                let pr = place.as_ref();
                out.push((self.place_node(pr.local, pr.projection), Kind::Ref));
            }
            Rvalue::BinaryOp(_, box (l, r)) => {
                if let Some(x) = self.process_operand(l) {
                    out.push(x);
                }
                if let Some(x) = self.process_operand(r) {
                    out.push(x);
                }
            }
            Rvalue::Aggregate(box kind, fields) => match kind {
                AggregateKind::RawPtr(_, _) => {
                    if let Some(first) = fields.iter().next() {
                        if let Some(x) = self.process_operand(first) {
                            out.push(x);
                        }
                    }
                }
                _ => {
                    for op in fields.iter() {
                        if let Some(x) = self.process_operand(op) {
                            out.push(x);
                        }
                    }
                }
            },
            _ => {}
        }
        out
    }

    fn process_assignment(&mut self, place: &Place<'tcx>, rvalue: &Rvalue<'tcx>) {
        let (lhs_loc, form) = self.process_place(place);
        let rhs_list = self.process_rvalue(rvalue);
        for (rhs_loc, kind) in rhs_list {
            for edge in assignment_edges(form, kind) {
                self.emit(edge, lhs_loc, rhs_loc);
            }
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
            let pr = destination.as_ref();
            self.place_node(pr.local, pr.projection)
        };

        let mut arg_nodes: SmallVec<[Option<LocId>; 4]> = SmallVec::new();
        for a in args {
            arg_nodes.push(self.process_operand(&a.node).map(|(loc, _)| loc));
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

    fn emit(&mut self, edge: EdgeKind, lhs: LocId, rhs: LocId) {
        let c = match edge {
            EdgeKind::AddressOf => Constraint::AddressOf { dst: lhs, obj: rhs },
            EdgeKind::Copy => Constraint::Copy { dst: lhs, src: rhs },
            EdgeKind::Load => Constraint::Load { dst: lhs, src: rhs },
            EdgeKind::Store => Constraint::Store { dst: lhs, src: rhs },
        };
        self.constraints.add(c);
    }

    /// Seed each distinct place node with its own abstract allocation object,
    /// mirroring `add_alloc` in the legacy analysis so dereferences of
    /// otherwise-unconstrained pointers still yield a concrete object.
    fn seed_allocs(&mut self) {
        let vars = std::mem::take(&mut self.vars);
        let empty = self.arena.empty_path();
        for (id, base, path) in vars {
            let site = AllocSite {
                func: self.func,
                bb: base,
                idx: path,
            };
            let obj = self.arena.heap(site, empty);
            self.constraints.add(Constraint::AddressOf { dst: id, obj });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EdgeKind::*;

    #[test]
    fn direct_ref_is_address_of() {
        assert_eq!(&assignment_edges(Form::Direct, Kind::Ref)[..], &[AddressOf]);
    }

    #[test]
    fn direct_direct_is_copy() {
        assert_eq!(&assignment_edges(Form::Direct, Kind::Direct)[..], &[Copy]);
    }

    #[test]
    fn direct_indirect_is_load() {
        assert_eq!(&assignment_edges(Form::Direct, Kind::Indirect)[..], &[Load]);
    }

    #[test]
    fn indirect_direct_is_store() {
        assert_eq!(&assignment_edges(Form::Indirect, Kind::Direct)[..], &[Store]);
    }

    #[test]
    fn indirect_ref_is_store_then_address_of() {
        assert_eq!(
            &assignment_edges(Form::Indirect, Kind::Ref)[..],
            &[Store, AddressOf]
        );
    }

    #[test]
    fn indirect_indirect_is_load() {
        assert_eq!(
            &assignment_edges(Form::Indirect, Kind::Indirect)[..],
            &[Load]
        );
    }
}
