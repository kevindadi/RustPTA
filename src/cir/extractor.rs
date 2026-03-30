//! Extract a native `CirArtifact` from `net::core::Net` (read-only).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::cir::types::{
    AnchorMap, BusinessGoal, CasOp, CirArtifact, CirFunction, CirOp, CirResource, CirStatement,
    CirTransfer, FunctionKind, ResourceKind, StoreOp, WaitOp, WriteOp,
};
use crate::net::core::Net;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::index_vec::Idx;
use crate::net::structure::{Place, PlaceType, TransitionType};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExtractionError {
    #[error("no entry function (no FunctionStart place with initial tokens > 0)")]
    NoEntryFunction,
    #[error("condvar {cv} has inconsistent paired mutex")]
    UnpairedCondvar { cv: String },
    #[error("disconnected control flow in {func}: unreachable place {place}")]
    DisconnectedControlFlow { func: String, place: String },
    #[error("ambiguous branch on transition {trans}: {count} outputs (expected 2)")]
    AmbiguousBranch { trans: String, count: usize },
    #[error("no matching FunctionEnd for start {0}")]
    MissingEnd(String),
}

#[derive(Debug, Clone)]
pub struct RawFunction {
    pub name: String,
    pub kind: FunctionKind,
    pub start_place: PlaceId,
    pub end_place: PlaceId,
    pub ordered_control_places: Vec<PlaceId>,
}

pub struct CirExtractor<'a> {
    net: &'a Net,
}

impl<'a> CirExtractor<'a> {
    pub fn new(net: &'a Net) -> Self {
        Self { net }
    }

    pub fn extract(&self) -> Result<CirArtifact, Vec<ExtractionError>> {
        let (mut resources, rid_to_name) = self.extract_resources();
        if let Err(e) = self.resolve_condvar_pairing(&mut resources, &rid_to_name) {
            return Err(e);
        }

        let raw_funcs = self.discover_functions();
        if raw_funcs.is_empty() {
            return Err(vec![ExtractionError::NoEntryFunction]);
        }

        let entry = self
            .find_entry_key(&raw_funcs)
            .ok_or_else(|| vec![ExtractionError::NoEntryFunction])?;

        let mut functions = BTreeMap::new();
        let mut all_errors = Vec::new();
        let mut anchor = AnchorMap {
            resource_id_to_name: rid_to_name.clone(),
            ..Default::default()
        };

        for rf in &raw_funcs {
            match self.linearize_function(rf, &rid_to_name) {
                Ok((cf, a)) => {
                    merge_anchor(&mut anchor, a);
                    functions.insert(rf.name.clone(), cf);
                }
                Err(e) => all_errors.extend(e),
            }
        }

        if !all_errors.is_empty() {
            return Err(all_errors);
        }

        anchor.resource_id_to_name = rid_to_name.clone();
        self.fill_resource_places(&mut anchor);

        let protection = self.infer_protection(&functions, &resources);
        let goals = self.generate_goals(&functions, &entry);

        Ok(CirArtifact {
            resources,
            protection,
            functions,
            goals,
            entry,
            anchor_map: anchor,
        })
    }

    fn extract_resources(&self) -> (BTreeMap<String, CirResource>, BTreeMap<usize, String>) {
        let mut rid_to_name: BTreeMap<usize, String> = BTreeMap::new();
        let mut resources: BTreeMap<String, CirResource> = BTreeMap::new();
        let mut mutex_ord: Vec<usize> = Vec::new();
        let mut rw_ord: Vec<usize> = Vec::new();
        let mut cv_ord: Vec<usize> = Vec::new();
        let mut atomic_fallback: usize = 0;
        let mut var_fallback: usize = 0;

        let name_for_rid =
            |rid: usize,
             ord: &mut Vec<usize>,
             prefix: &str,
             rid_to_name: &mut BTreeMap<usize, String>,
             resources: &mut BTreeMap<String, CirResource>,
             kind: ResourceKind| {
                if let Some(n) = rid_to_name.get(&rid) {
                    return n.clone();
                }
                let idx = if let Some(i) = ord.iter().position(|x| *x == rid) {
                    i
                } else {
                    ord.push(rid);
                    ord.len() - 1
                };
                let name = format!("{}{}", prefix, idx);
                rid_to_name.insert(rid, name.clone());
                resources.insert(
                    name.clone(),
                    CirResource {
                        kind,
                        paired_with: None,
                        permits: None,
                        capacity: None,
                        var_type: None,
                        init: None,
                        span: None,
                    },
                );
                name
            };

        for (_, t) in self.net.transitions.iter_enumerated() {
            match &t.transition_type {
                TransitionType::Lock(r) | TransitionType::Unlock(r) => {
                    name_for_rid(
                        *r,
                        &mut mutex_ord,
                        "m",
                        &mut rid_to_name,
                        &mut resources,
                        ResourceKind::Mutex,
                    );
                }
                TransitionType::RwLockRead(r)
                | TransitionType::RwLockWrite(r)
                | TransitionType::DropRead(r)
                | TransitionType::DropWrite(r) => {
                    name_for_rid(
                        *r,
                        &mut rw_ord,
                        "rw",
                        &mut rid_to_name,
                        &mut resources,
                        ResourceKind::RwLock,
                    );
                }
                TransitionType::Notify(r) => {
                    name_for_rid(
                        *r,
                        &mut cv_ord,
                        "cv",
                        &mut rid_to_name,
                        &mut resources,
                        ResourceKind::Condvar,
                    );
                }
                TransitionType::AtomicLoad(_, _, s, rid)
                | TransitionType::AtomicStore(_, _, s, rid)
                | TransitionType::AtomicCmpXchg(_, _, _, s, rid) => {
                    if rid_to_name.contains_key(rid) {
                        continue;
                    }
                    let nm = if s.is_empty() {
                        let n = format!("a{}", atomic_fallback);
                        atomic_fallback += 1;
                        n
                    } else {
                        sanitize_key(s)
                    };
                    rid_to_name.insert(*rid, nm.clone());
                    resources.insert(
                        nm.clone(),
                        CirResource {
                            kind: ResourceKind::Atomic,
                            paired_with: None,
                            permits: None,
                            capacity: None,
                            var_type: Some("Int".into()),
                            init: None,
                            span: None,
                        },
                    );
                }
                TransitionType::UnsafeRead(rid, s, _, _) | TransitionType::UnsafeWrite(rid, s, _, _) => {
                    if rid_to_name.contains_key(rid) {
                        continue;
                    }
                    let nm = if s.is_empty() {
                        let n = format!("v{}", var_fallback);
                        var_fallback += 1;
                        n
                    } else {
                        sanitize_key(s)
                    };
                    rid_to_name.insert(*rid, nm.clone());
                    resources.insert(
                        nm.clone(),
                        CirResource {
                            kind: ResourceKind::Var,
                            paired_with: None,
                            permits: None,
                            capacity: None,
                            var_type: Some("Int".into()),
                            init: None,
                            span: None,
                        },
                    );
                }
                _ => {}
            }
        }

        (resources, rid_to_name)
    }

    fn resolve_condvar_pairing(
        &self,
        resources: &mut BTreeMap<String, CirResource>,
        rid_to_name: &BTreeMap<usize, String>,
    ) -> Result<(), Vec<ExtractionError>> {
        let place_to_mutex_rid = self.mutex_place_to_rid();
        let mut cv_to_mx: BTreeMap<usize, usize> = BTreeMap::new();
        let mut errs = Vec::new();

        for (tid, t) in self.net.transitions.iter_enumerated() {
            if !matches!(t.transition_type, TransitionType::Wait) {
                continue;
            }
            let Some(cv_rid) = self.condvar_rid_for_wait_tid(tid) else {
                continue;
            };
            let Some(mx_rid) = self.output_mutex_rid_for_tid(tid, &place_to_mutex_rid) else {
                errs.push(ExtractionError::UnpairedCondvar {
                    cv: rid_to_name
                        .get(&cv_rid)
                        .cloned()
                        .unwrap_or_else(|| format!("cv{}", cv_rid)),
                });
                continue;
            };
            if let Some(prev) = cv_to_mx.get(&cv_rid) {
                if prev != &mx_rid {
                    errs.push(ExtractionError::UnpairedCondvar {
                        cv: rid_to_name
                            .get(&cv_rid)
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            } else {
                cv_to_mx.insert(cv_rid, mx_rid);
            }
        }

        if cv_to_mx.is_empty() && errs.is_empty() {
            return Ok(());
        }

        for (cv_rid, mx_rid) in cv_to_mx {
            let cv_name = rid_to_name
                .get(&cv_rid)
                .cloned()
                .unwrap_or_else(|| format!("cv{}", cv_rid));
            let mx_name = rid_to_name
                .get(&mx_rid)
                .cloned()
                .unwrap_or_else(|| format!("m{}", mx_rid));
            if let Some(res) = resources.get_mut(&cv_name) {
                res.paired_with = Some(mx_name);
            }
        }

        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }

    fn mutex_place_to_rid(&self) -> BTreeMap<PlaceId, usize> {
        let mut m = BTreeMap::new();
        for (tid, t) in self.net.transitions.iter_enumerated() {
            let r = match &t.transition_type {
                TransitionType::Lock(r) => *r,
                _ => continue,
            };
            for (pid, pl) in self.net.places.iter_enumerated() {
                if pl.place_type == PlaceType::Resources && *self.net.pre.get(pid, tid) > 0 {
                    m.insert(pid, r);
                }
            }
        }
        m
    }

    fn condvar_place_to_rid(&self) -> BTreeMap<PlaceId, usize> {
        let mut m = BTreeMap::new();
        for (tid, t) in self.net.transitions.iter_enumerated() {
            let r = match &t.transition_type {
                TransitionType::Notify(rid) => *rid,
                _ => continue,
            };
            for (pid, pl) in self.net.places.iter_enumerated() {
                if pl.place_type == PlaceType::Resources && *self.net.pre.get(pid, tid) > 0 {
                    m.insert(pid, r);
                }
            }
        }
        m
    }

    fn condvar_rid_for_wait_tid(&self, wait_tid: TransitionId) -> Option<usize> {
        let cmap = self.condvar_place_to_rid();
        for (pid, pl) in self.net.places.iter_enumerated() {
            if pl.place_type != PlaceType::Resources {
                continue;
            }
            if *self.net.pre.get(pid, wait_tid) > 0 {
                if let Some(r) = cmap.get(&pid) {
                    return Some(*r);
                }
            }
        }
        None
    }

    fn output_mutex_rid_for_tid(
        &self,
        tid: TransitionId,
        place_to_mutex_rid: &BTreeMap<PlaceId, usize>,
    ) -> Option<usize> {
        for (pid, _) in self.net.places.iter_enumerated() {
            if *self.net.post.get(pid, tid) > 0 {
                if let Some(r) = place_to_mutex_rid.get(&pid) {
                    return Some(*r);
                }
            }
        }
        None
    }

    fn make_wait_op(&self, tid: TransitionId, rid_to_name: &BTreeMap<usize, String>) -> Option<CirOp> {
        let place_to_mutex = self.mutex_place_to_rid();
        let mx_rid = self.output_mutex_rid_for_tid(tid, &place_to_mutex)?;
        let cv_rid = self.condvar_rid_for_wait_tid(tid)?;
        let rn = |rid: usize| -> String {
            rid_to_name
                .get(&rid)
                .cloned()
                .unwrap_or_else(|| format!("r{}", rid))
        };
        Some(CirOp::Wait {
            wait: WaitOp {
                cv: rn(cv_rid),
                mutex: rn(mx_rid),
            },
        })
    }

    fn discover_functions(&self) -> Vec<RawFunction> {
        let mut out = Vec::new();
        for (pid, pl) in self.net.places.iter_enumerated() {
            if pl.place_type != PlaceType::FunctionStart {
                continue;
            }
            let key = pl
                .name
                .strip_suffix("_start")
                .unwrap_or(&pl.name)
                .to_string();
            let fk = function_display_key(&key);
            let end_name = format!("{}_end", key);
            let Some((end_id, _)) = self
                .net
                .places
                .iter_enumerated()
                .find(|(_, p)| p.name == end_name && p.place_type == PlaceType::FunctionEnd)
            else {
                continue;
            };
            let kind = self.infer_function_kind(pid, end_id, &fk);
            let ordered = self.control_place_order(pid, end_id);
            out.push(RawFunction {
                name: fk,
                kind,
                start_place: pid,
                end_place: end_id,
                ordered_control_places: ordered,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn infer_function_kind(&self, start: PlaceId, end: PlaceId, name: &str) -> FunctionKind {
        let mut async_seen = false;
        for (tid, t) in self.net.transitions.iter_enumerated() {
            if !self.transition_on_path(tid, start, end) {
                continue;
            }
            match &t.transition_type {
                TransitionType::AsyncSpawn { .. }
                | TransitionType::AsyncJoin { .. }
                | TransitionType::AwaitPending { .. }
                | TransitionType::AwaitReady { .. } => async_seen = true,
                _ => {}
            }
        }
        if async_seen {
            return FunctionKind::Async;
        }
        let mut spawned = false;
        for (_, t) in self.net.transitions.iter_enumerated() {
            if let TransitionType::Spawn(s) = &t.transition_type {
                if function_display_key(s) == name {
                    spawned = true;
                }
            }
        }
        if name.contains("closure") || spawned {
            FunctionKind::Closure
        } else {
            FunctionKind::Normal
        }
    }

    fn transition_on_path(&self, tid: TransitionId, start: PlaceId, end: PlaceId) -> bool {
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            if !seen.insert(p) {
                continue;
            }
            if p == end {
                continue;
            }
            for (tix, _) in self.net.transitions.iter_enumerated() {
                if *self.net.pre.get(p, tix) == 0 {
                    continue;
                }
                if tix == tid {
                    return true;
                }
                for (p2, pl2) in self.net.places.iter_enumerated() {
                    if *self.net.post.get(p2, tix) > 0 && self.is_control_place(pl2) {
                        q.push_back(p2);
                    }
                }
            }
        }
        false
    }

    fn find_entry_key(&self, raw: &[RawFunction]) -> Option<String> {
        for rf in raw {
            let pl = &self.net.places[rf.start_place];
            if pl.tokens >0 && rf.name == "main" {
                return Some("main".into());
            }
        }
        raw.iter()
            .find(|rf| self.net.places[rf.start_place].tokens > 0)
            .map(|rf| rf.name.clone())
    }

    fn control_place_order(&self, start: PlaceId, end: PlaceId) -> Vec<PlaceId> {
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            if !visited.insert(p) {
                continue;
            }
            order.push(p);
            if p == end {
                break;
            }
            let mut next_places: Vec<(PlaceId, u32)> = Vec::new();
            for (tid, _) in self.net.transitions.iter_enumerated() {
                if *self.net.pre.get(p, tid) == 0 {
                    continue;
                }
                for (p2, pl2) in self.net.places.iter_enumerated() {
                    if *self.net.post.get(p2, tid) == 0 || !self.is_control_place(pl2) {
                        continue;
                    }
                    next_places.push((p2, bb_ord(&pl2.name)));
                }
            }
            next_places.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            for (p2, _) in next_places {
                if !visited.contains(&p2) {
                    q.push_back(p2);
                }
            }
        }
        if !visited.contains(&end) {
            order.push(end);
        }
        order
    }

    fn is_control_place(&self, p: &Place) -> bool {
        matches!(
            p.place_type,
            PlaceType::BasicBlock | PlaceType::FunctionStart | PlaceType::FunctionEnd
        )
    }

    fn linearize_function(
        &self,
        rf: &RawFunction,
        rid_to_name: &BTreeMap<usize, String>,
    ) -> Result<(CirFunction, AnchorMap), Vec<ExtractionError>> {
        let prefix = sid_prefix(&rf.name);
        let mut anchor = AnchorMap {
            resource_id_to_name: rid_to_name.clone(),
            ..Default::default()
        };
        let path = self.shortest_control_path(rf.start_place, rf.end_place);

        let mut sid_n = 0usize;
        let mut statements: Vec<(String, CirOp, Option<TransitionId>, Option<PlaceId>, CirTransfer)> =
            Vec::new();

        if path.len() >= 2 {
            for w in path.windows(2) {
                let from = w[0];
                let to = w[1];
                let mut tids = self.transitions_between_control(from, to);
                tids.sort_by_key(|t| t.index());
                for tid in tids {
                    let t = &self.net.transitions[tid];
                    if matches!(
                        t.transition_type,
                        TransitionType::Goto | TransitionType::Normal | TransitionType::Return(_)
                    ) {
                        continue;
                    }
                    let op = if matches!(t.transition_type, TransitionType::Wait) {
                        if let Some(o) = self.make_wait_op(tid, rid_to_name) {
                            o
                        } else {
                            continue;
                        }
                    } else {
                        match transition_to_cir_op(t, rid_to_name) {
                            Some(o) => o,
                            None => continue,
                        }
                    };
                    if matches!(op, CirOp::Return) {
                        continue;
                    }
                    let sid = format!("{}{}", prefix, sid_n);
                    sid_n += 1;
                    statements.push((
                        sid,
                        op,
                        Some(tid),
                        Some(from),
                        CirTransfer::Next {
                            next: String::new(),
                        },
                    ));
                }
            }
        }

        for i in 0..statements.len() {
            let next_sid = if i + 1 < statements.len() {
                statements[i + 1].0.clone()
            } else {
                "ret".to_string()
            };
            if let CirTransfer::Next { next } = &mut statements[i].4 {
                *next = next_sid;
            }
        }

        let mut body: Vec<CirStatement> = Vec::new();
        for (sid, op, tid, from_pl, xfer) in statements {
            let span = from_pl.and_then(|p| {
                let s = self.net.places[p].span.clone();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            });
            body.push(CirStatement {
                sid: sid.clone(),
                op,
                transfer: xfer,
                span,
            });
            if let Some(p) = from_pl {
                anchor.sid_to_place.insert(sid.clone(), p.index());
                anchor.place_to_sid.insert(p.index(), sid.clone());
            }
            if let Some(t) = tid {
                anchor.sid_to_transition.insert(sid.clone(), t.index());
                anchor.transition_to_sid.insert(t.index(), sid);
            }
        }

        let sid_ret = "ret".to_string();
        body.push(CirStatement {
            sid: sid_ret.clone(),
            op: CirOp::Return,
            transfer: CirTransfer::Done { done: true },
            span: None,
        });
        anchor
            .sid_to_place
            .insert(sid_ret.clone(), rf.end_place.index());
        anchor
            .place_to_sid
            .insert(rf.end_place.index(), sid_ret.clone());

        Ok((
            CirFunction {
                kind: rf.kind.clone(),
                body,
            },
            anchor,
        ))
    }

    fn shortest_control_path(&self, start: PlaceId, end: PlaceId) -> Vec<PlaceId> {
        let mut prev: BTreeMap<PlaceId, Option<PlaceId>> = BTreeMap::new();
        let mut q = VecDeque::new();
        q.push_back(start);
        prev.insert(start, None);
        while let Some(p) = q.pop_front() {
            if p == end {
                break;
            }
            for np in self.control_successors_sorted(p) {
                if prev.contains_key(&np) {
                    continue;
                }
                prev.insert(np, Some(p));
                q.push_back(np);
            }
        }
        if !prev.contains_key(&end) {
            return vec![start, end];
        }
        let mut path = vec![end];
        let mut cur = end;
        while let Some(Some(b)) = prev.get(&cur).copied() {
            path.push(b);
            cur = b;
            if cur == start {
                break;
            }
        }
        path.reverse();
        path
    }

    fn control_successors_sorted(&self, p: PlaceId) -> Vec<PlaceId> {
        let mut nxt: Vec<(PlaceId, u32)> = Vec::new();
        for (tid, t) in self.net.transitions.iter_enumerated() {
            if *self.net.pre.get(p, tid) == 0 {
                continue;
            }
            if matches!(t.transition_type, TransitionType::Assert) {
                continue;
            }
            for (p2, pl2) in self.net.places.iter_enumerated() {
                if *self.net.post.get(p2, tid) == 0 || !self.is_control_place(pl2) {
                    continue;
                }
                nxt.push((p2, bb_ord(&pl2.name)));
            }
        }
        nxt.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        nxt.into_iter().map(|(p, _)| p).collect()
    }

    fn transitions_between_control(&self, from: PlaceId, to: PlaceId) -> Vec<TransitionId> {
        let mut out = Vec::new();
        for (tid, _) in self.net.transitions.iter_enumerated() {
            if *self.net.pre.get(from, tid) == 0 || *self.net.post.get(to, tid) == 0 {
                continue;
            }
            out.push(tid);
        }
        out
    }

    fn fill_resource_places(&self, anchor: &mut AnchorMap) {
        let mmap = self.mutex_place_to_rid();
        let cmap = self.condvar_place_to_rid();
        for (pid, rid) in mmap.into_iter().chain(cmap) {
            let name = anchor
                .resource_id_to_name
                .get(&rid)
                .cloned()
                .unwrap_or_else(|| format!("r{}", rid));
            anchor
                .resource_to_places
                .entry(name)
                .or_default()
                .push(pid.index());
        }
        for v in anchor.resource_to_places.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
    }

    fn infer_protection(
        &self,
        functions: &BTreeMap<String, CirFunction>,
        resources: &BTreeMap<String, CirResource>,
    ) -> BTreeMap<String, Vec<String>> {
        let mut prot: BTreeMap<String, Vec<BTreeSet<String>>> = BTreeMap::new();
        for (_fname, cf) in functions {
            let mut held: BTreeSet<String> = BTreeSet::new();
            for st in &cf.body {
                Self::apply_op_holds_static(&st.op, &mut held);
                if let CirOp::Read { read } = &st.op {
                    if resources.get(read).map(|r| r.kind == ResourceKind::Var) == Some(true) {
                        prot.entry(read.clone()).or_default().push(held.clone());
                    }
                }
                if let CirOp::Write { write } = &st.op {
                    let v = write.var.clone();
                    if resources.get(&v).map(|r| r.kind == ResourceKind::Var) == Some(true) {
                        prot.entry(v).or_default().push(held.clone());
                    }
                }
            }
        }
        let mut out = BTreeMap::new();
        for (var, sets) in prot {
            if sets.is_empty() {
                continue;
            }
            let inter: BTreeSet<_> = sets
                .iter()
                .skip(1)
                .fold(sets[0].clone(), |acc, s| acc.intersection(s).cloned().collect());
            if inter.len() == 1 {
                out.insert(var, vec![inter.iter().next().unwrap().clone()]);
            }
        }
        out
    }

    fn apply_op_holds_static(op: &CirOp, held: &mut BTreeSet<String>) {
        match op {
            CirOp::Lock { lock } | CirOp::WriteLock { write_lock: lock } => {
                held.insert(lock.clone());
            }
            CirOp::ReadLock { read_lock } => {
                held.insert(read_lock.clone());
            }
            CirOp::Drop { drop } => {
                held.remove(drop);
            }
            CirOp::Wait { wait } => {
                held.insert(wait.mutex.clone());
            }
            _ => {}
        }
    }

    fn generate_goals(&self, functions: &BTreeMap<String, CirFunction>, _entry: &str) -> Vec<BusinessGoal> {
        let mut spawned = BTreeSet::new();
        for f in functions.values() {
            for st in &f.body {
                if let CirOp::Spawn { spawn } = &st.op {
                    spawned.insert(spawn.clone());
                }
            }
        }
        let mut goals = Vec::new();
        let mut gid = 0u32;
        goals.push(BusinessGoal {
            id: format!("G{}", gid),
            desc: "main completes".into(),
            marking: bt("main"),
            variables: BTreeMap::new(),
        });
        gid += 1;
        for s in spawned {
            goals.push(BusinessGoal {
                id: format!("G{}", gid),
                desc: format!("{} completes", s),
                marking: bt(&s),
                variables: BTreeMap::new(),
            });
            gid += 1;
        }
        goals
    }
}

fn bt(func: &str) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    m.insert(format!("cp({}, ret)", func), 1);
    m
}

fn merge_anchor(dst: &mut AnchorMap, src: AnchorMap) {
    dst.resource_to_places.extend(src.resource_to_places);
    dst.sid_to_place.extend(src.sid_to_place);
    dst.place_to_sid.extend(src.place_to_sid);
    dst.sid_to_transition.extend(src.sid_to_transition);
    dst.transition_to_sid.extend(src.transition_to_sid);
}

fn sid_prefix(func_name: &str) -> String {
    let mut cs = func_name.chars().filter(|c| c.is_alphanumeric());
    let mut s = cs.next().unwrap_or('f').to_string();
    if let Some(c2) = cs.next() {
        s.push(c2);
    }
    s
}

fn function_display_key(full: &str) -> String {
    full.rsplit("::").next().unwrap_or(full).to_string()
}

fn sanitize_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn bb_ord(name: &str) -> u32 {
    name.rsplit('_')
        .next()
        .and_then(|t| t.parse().ok())
        .unwrap_or(0)
}

fn transition_to_cir_op(
    t: &crate::net::structure::Transition,
    rid_to_name: &BTreeMap<usize, String>,
) -> Option<CirOp> {
    let rn = |rid: &usize| rid_to_name.get(rid).cloned().unwrap_or_else(|| format!("m{}", rid));
    Some(match &t.transition_type {
        TransitionType::Lock(r) => CirOp::Lock { lock: rn(r) },
        TransitionType::Unlock(r) | TransitionType::DropRead(r) | TransitionType::DropWrite(r) => {
            CirOp::Drop { drop: rn(r) }
        }
        TransitionType::RwLockRead(r) => CirOp::ReadLock {
            read_lock: rn(r),
        },
        TransitionType::RwLockWrite(r) => CirOp::WriteLock {
            write_lock: rn(r),
        },
        TransitionType::Drop => return None,
        TransitionType::Wait => return None,
        TransitionType::Notify(r) => CirOp::NotifyOne {
            notify_one: rn(r),
        },
        TransitionType::AtomicLoad(_, _, _, rid) => CirOp::Load { load: rn(rid) },
        TransitionType::AtomicStore(_, _, _, rid) => CirOp::Store {
            store: StoreOp {
                var: rn(rid),
                val: "unknown".into(),
            },
        },
        TransitionType::AtomicCmpXchg(_, _, _, _, rid) => CirOp::Cas {
            cas: CasOp {
                var: rn(rid),
                expected: "unknown".into(),
                new: "unknown".into(),
            },
        },
        TransitionType::UnsafeRead(rid, n, _, _) => CirOp::Read {
            read: rid_to_name.get(rid).cloned().unwrap_or_else(|| n.clone()),
        },
        TransitionType::UnsafeWrite(rid, n, _, _) => CirOp::Write {
            write: WriteOp {
                var: rid_to_name.get(rid).cloned().unwrap_or_else(|| n.clone()),
                val: "unknown".into(),
            },
        },
        TransitionType::Spawn(s) => CirOp::Spawn {
            spawn: function_display_key(s),
        },
        TransitionType::Join(s) => CirOp::Join {
            join: function_display_key(s),
        },
        TransitionType::Function => CirOp::Call {
            call: t.name.clone(),
        },
        TransitionType::Return(_) => return None,
        _ => return None,
    })
}
