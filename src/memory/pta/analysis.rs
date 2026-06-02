//! Whole-program driver.
//!
//! [`PointerAnalysis`] builds constraints for all functions reachable (by
//! direct calls) from a set of roots, performing interprocedural binding for
//! analyzable callees and falling back to the conservative model otherwise,
//! then solves to a [`PointsToResult`]. Context-insensitive (`k = 0`) for now;
//! the `ContextPolicy` hook is ready for k-CFA in a later phase.

extern crate rustc_middle;

use std::collections::VecDeque;

use rustc_data_structures::fx::FxHashSet;
use rustc_middle::ty::{Instance, TyCtxt, TypingEnv};

use super::builder::{build_body, PendingCall};
use super::constraint::ConstraintSet;
use super::interproc::{bind_call_edges, FuncMap};
use super::loc::{AbstractLoc, FieldPath, LocArena, LocId, ProjElem};
use super::model::{CallNodes, ModelRegistry};
use super::result::PointsToResult;
use super::solver::Solver;

pub struct PointerAnalysis<'tcx> {
    tcx: TyCtxt<'tcx>,
    arena: LocArena,
    constraints: ConstraintSet,
    funcs: FuncMap<'tcx>,
    registry: ModelRegistry,
    built: FxHashSet<Instance<'tcx>>,
}

impl<'tcx> PointerAnalysis<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self {
            tcx,
            arena: LocArena::default(),
            constraints: ConstraintSet::default(),
            funcs: FuncMap::default(),
            registry: ModelRegistry::builtin(),
            built: FxHashSet::default(),
        }
    }

    /// Mutable access to the model registry for registering extra models.
    pub fn registry_mut(&mut self) -> &mut ModelRegistry {
        &mut self.registry
    }

    /// Build constraints for every function reachable by direct calls from
    /// `roots`. Safe to call multiple times; already-built functions are skipped.
    pub fn build_reachable<I>(&mut self, roots: I)
    where
        I: IntoIterator<Item = Instance<'tcx>>,
    {
        let mut queue: VecDeque<Instance<'tcx>> = roots.into_iter().collect();
        while let Some(inst) = queue.pop_front() {
            if !self.built.insert(inst) {
                continue;
            }
            if !self.tcx.is_mir_available(inst.def_id()) {
                continue;
            }
            let body = self.tcx.instance_mir(inst.def);
            if body.source.promoted.is_some() {
                continue;
            }
            let func = self.funcs.intern(inst);
            let pending = build_body(
                self.tcx,
                body,
                func,
                inst,
                &self.registry,
                &mut self.arena,
                &mut self.constraints,
            );
            for pc in pending {
                self.resolve_pending(inst, pc, &mut queue);
            }
        }
    }

    fn resolve_pending(
        &mut self,
        caller: Instance<'tcx>,
        pc: PendingCall<'tcx>,
        queue: &mut VecDeque<Instance<'tcx>>,
    ) {
        if let Some((def_id, substs)) = pc.callee {
            let typing_env = TypingEnv::post_analysis(self.tcx, caller.def_id());
            let resolved = Instance::try_resolve(self.tcx, typing_env, def_id, substs)
                .ok()
                .flatten();
            if let Some(callee) = resolved {
                if self.tcx.is_mir_available(callee.def_id()) {
                    let body = self.tcx.instance_mir(callee.def);
                    if body.source.promoted.is_none() {
                        self.bind_callee(callee, body.arg_count, &pc);
                        if !self.built.contains(&callee) {
                            queue.push_back(callee);
                        }
                        return;
                    }
                }
            }
        }
        // Indirect, unresolved, or no MIR available: conservative model.
        self.apply_unknown(pc);
    }

    /// Emit interprocedural binding constraints between a call site and an
    /// analyzable callee (`param ⊇ arg`, `dest ⊇ callee._0`).
    fn bind_callee(&mut self, callee: Instance<'tcx>, arg_count: usize, pc: &PendingCall<'tcx>) {
        let callee_func = self.funcs.intern(callee);
        let empty = self.arena.empty_path();
        let mut params: Vec<LocId> = Vec::with_capacity(arg_count);
        for i in 1..=arg_count {
            params.push(self.arena.var(callee_func, i as u32, empty));
        }
        let ret = self.arena.var(callee_func, 0, empty);
        for edge in bind_call_edges(&params, ret, &pc.args, pc.dest) {
            self.constraints.add(edge);
        }
    }

    fn apply_unknown(&mut self, pc: PendingCall<'tcx>) {
        let nodes = CallNodes {
            dest: pc.dest,
            args: pc.args,
            fresh_heap: pc.fresh_heap,
        };
        self.registry.apply_unknown(&nodes, &mut self.constraints);
    }

    /// Solve the accumulated constraints and return a query facade.
    pub fn solve(&self) -> PointsToResult {
        let pts = Solver::new(self.arena.loc_count()).solve(&self.constraints);
        PointsToResult::new(pts)
    }

    /// The interned `LocId` of a function-local variable (interns if needed).
    /// `local = 0` is the return place; `1..=arg_count` are parameters.
    pub fn node_of(&mut self, instance: Instance<'tcx>, local: u32) -> LocId {
        let func = self.funcs.intern(instance);
        let empty = self.arena.empty_path();
        self.arena.var(func, local, empty)
    }

    pub fn arena(&self) -> &LocArena {
        &self.arena
    }

    pub fn constraints(&self) -> &ConstraintSet {
        &self.constraints
    }

    /// Render the solved points-to relation as a deterministic, human-readable
    /// report. Used for differential comparison against the legacy engine.
    pub fn format_report(&self) -> String {
        let result = self.solve();
        let mut entries: Vec<(LocId, Vec<LocId>)> = result
            .raw()
            .raw()
            .iter()
            .filter(|(_, set)| !set.is_empty())
            .map(|(node, set)| {
                let mut pointees: Vec<LocId> = set.iter().copied().collect();
                pointees.sort_unstable();
                (*node, pointees)
            })
            .collect();
        entries.sort_unstable_by_key(|(node, _)| *node);

        let mut out = String::from("=== PTA Points-To Report (new engine) ===\n");
        out.push_str(&format!("nodes-with-pointees: {}\n", entries.len()));
        for (node, pointees) in &entries {
            let lhs = self.fmt_loc(*node);
            let rhs: Vec<String> = pointees.iter().map(|p| self.fmt_loc(*p)).collect();
            out.push_str(&format!("  {} -> {{ {} }}\n", lhs, rhs.join(", ")));
        }
        out.push_str("=== End ===\n");
        out
    }

    fn fmt_loc(&self, id: LocId) -> String {
        match self.arena.loc(id) {
            AbstractLoc::Var { func, base, path, .. } => {
                format!("f{}::_{}{}", func, base, self.fmt_path(*path))
            }
            AbstractLoc::Heap { site, path, .. } => {
                format!(
                    "Heap(f{}:bb{}#{}){}",
                    site.func,
                    site.bb,
                    site.idx,
                    self.fmt_path(*path)
                )
            }
            AbstractLoc::Global { def_index, path } => {
                format!("Global({}){}", def_index, self.fmt_path(*path))
            }
        }
    }

    fn fmt_path(&self, path: FieldPath) -> String {
        let mut s = String::new();
        for elem in self.arena.path(path) {
            match elem {
                ProjElem::Field(i) => s.push_str(&format!(".{}", i)),
                ProjElem::Deref => s.push_str(".*"),
                ProjElem::Index => s.push_str("[*]"),
            }
        }
        s
    }
}
