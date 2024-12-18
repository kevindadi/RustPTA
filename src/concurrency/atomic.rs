//! Find atomic functions and classify them into read, write, read-write.
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_middle;

use once_cell::sync::Lazy;
use petgraph::visit::{IntoNodeReferences, NodeRef};
use regex::Regex;
use rustc_abi::VariantIdx;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    visit::Visitor, Body, Local, Location, Operand, Place, Terminator, TerminatorKind,
};
use rustc_middle::ty::{self, GenericArg, Instance, List, Ty, TyCtxt, TyKind, TypingEnv};
use serde_json::json;

use crate::graph::callgraph::{CallGraph, CallGraphNode};
use crate::utils::format_name;

static ATOMIC_PTR_STORE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(std|core)::sync::atomic::AtomicPtr::<.*>::store").unwrap());

pub fn is_atomic_ptr_store<'tcx>(
    def_id: DefId,
    substs: &'tcx List<GenericArg<'tcx>>,
    tcx: TyCtxt<'tcx>,
) -> bool {
    let path = tcx.def_path_str_with_args(def_id, substs);
    ATOMIC_PTR_STORE.is_match(&path)
}

macro_rules! is_atomic_api {
    ($fn_name:expr, "load") => {
        $fn_name.contains("::load")
    };
    ($fn_name:expr, "store") => {
        $fn_name.contains("::store")
    };
    ($fn_name:expr, "compare_exchange") => {
        $fn_name.contains("::compare_exchange")
    };
    ($fn_name:expr, "fetch_add") => {
        $fn_name.contains("::fetch_add")
    };
    ($fn_name:expr, "fetch_sub") => {
        $fn_name.contains("::fetch_sub")
    };
    ($fn_name:expr, "fetch_and") => {
        $fn_name.contains("::fetch_and")
    };
    ($fn_name:expr, "fetch_or") => {
        $fn_name.contains("::fetch_or")
    };
    ($fn_name:expr, "fetch_xor") => {
        $fn_name.contains("::fetch_xor")
    };
    ($fn_name:expr, "swap") => {
        $fn_name.contains("::swap")
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicApi {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AtomicOrdering {
    Relaxed,
    Release,
    Acquire,
    AcqRel,
    SeqCst,
}

impl AtomicOrdering {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => AtomicOrdering::Relaxed,
            1 => AtomicOrdering::Release,
            2 => AtomicOrdering::Acquire,
            3 => AtomicOrdering::AcqRel,
            4 => AtomicOrdering::SeqCst,
            _ => AtomicOrdering::SeqCst,
        }
    }
    pub fn from_u128(value: u128) -> Self {
        match value {
            0 => AtomicOrdering::Relaxed,
            1 => AtomicOrdering::Release,
            2 => AtomicOrdering::Acquire,
            3 => AtomicOrdering::AcqRel,
            4 => AtomicOrdering::SeqCst,
            _ => AtomicOrdering::SeqCst,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AtomicOperation {
    pub api: AtomicApi,
    pub ordering: AtomicOrdering,
    pub location: String,
}

#[derive(Debug, Clone)]
pub struct AtomicVarInfo {
    pub var_type: String,
    pub span: String,
    pub operations: Vec<AtomicOperation>,
}

pub type AtomicVarMap = FxHashMap<String, AtomicVarInfo>;

pub struct AtomicCollector<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
    crate_name: String,
    pub atomic_vars: AtomicVarMap,
}

impl<'a, 'tcx> AtomicCollector<'a, 'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, callgraph: &'a CallGraph<'tcx>, crate_name: String) -> Self {
        Self {
            tcx,
            callgraph,
            crate_name,
            atomic_vars: AtomicVarMap::default(),
        }
    }

    pub fn analyze(&mut self) -> AtomicVarMap {
        // 遍历callgraph中的所有函数
        for node_ref in self.callgraph.graph.node_references() {
            if let CallGraphNode::WithBody(instance) = node_ref.weight() {
                let def_id = instance.def_id();
                // 只分析当前crate的函数
                if def_id.is_local() && format_name(def_id).starts_with(&self.crate_name) {
                    if self.tcx.is_mir_available(def_id) {
                        let body = self.tcx.optimized_mir(def_id);
                        self.collect_atomic_vars(instance, body);
                    }
                }
            }
        }
        self.atomic_vars.clone()
    }

    fn collect_atomic_vars(&mut self, instance: &Instance<'tcx>, body: &Body<'tcx>) {
        // 收集atomic类型的局部变量
        for (local, local_decl) in body.local_decls.iter_enumerated() {
            let ty = local_decl.ty;
            if self.is_atomic_type(ty) && !ty.to_string().contains("Ordering") {
                let var_name = format!(
                    "{}_{}",
                    self.tcx.def_path_str(instance.def_id()),
                    local.index()
                );
                let info = AtomicVarInfo {
                    var_type: ty.to_string(),
                    span: format!("{:?}", local_decl.source_info.span),
                    operations: Vec::new(),
                };
                self.atomic_vars.insert(var_name, info);
            }
        }

        // 遍历MIR收集操作
        let mut visitor = AtomicVisitor {
            instance: *instance,
            body,
            tcx: self.tcx,
            atomic_vars: &mut self.atomic_vars,
        };
        visitor.visit_body(body);
    }

    fn is_atomic_type(&self, ty: Ty<'tcx>) -> bool {
        if let TyKind::Adt(adt_def, _) = ty.kind() {
            let path = self.tcx.def_path_str(adt_def.did());
            path.contains("::sync::atomic::")
        } else {
            false
        }
    }

    #[allow(dead_code)]
    fn to_json_pretty(&self) -> Result<(), serde_json::Error> {
        if self.atomic_vars.is_empty() {
            log::debug!("No atomic variables found");
        } else {
            for (var_name, info) in self.atomic_vars.iter() {
                log::info!(
                    "Atomic Variable {}:\n{}",
                    var_name,
                    serde_json::to_string_pretty(&json!({
                        "type": info.var_type,
                        "defined_at": info.span,
                        "operations": info.operations
                            .iter()
                            .map(|op| json!({
                                "api": format!("{:?}", op.api),
                                "ordering": format!("{:?}", op.ordering),
                                "location": op.location
                            }))
                            .collect::<Vec<_>>()
                    }))
                    .unwrap()
                );
            }
        }
        Ok(())
    }
}

struct AtomicVisitor<'a, 'tcx> {
    instance: Instance<'tcx>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    pub atomic_vars: &'a mut AtomicVarMap,
}

impl<'a, 'tcx> Visitor<'tcx> for AtomicVisitor<'a, 'tcx> {
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Call { func, args, .. } = &terminator.kind {
            let func_ty = func.ty(self.body, self.tcx);
            if let TyKind::FnDef(def_id, _) = func_ty.kind() {
                let fn_name = self.tcx.def_path_str(*def_id);

                // 先判断是否是atomic操作
                let api = if fn_name.contains("::load") {
                    Some(AtomicApi::Read)
                } else if fn_name.contains("::store") {
                    Some(AtomicApi::Write)
                } else if fn_name.contains("::compare_exchange")
                    || fn_name.contains("::fetch_add")
                    || fn_name.contains("::fetch_sub")
                    || fn_name.contains("::fetch_and")
                    || fn_name.contains("::fetch_or")
                    || fn_name.contains("::fetch_xor")
                    || fn_name.contains("::swap")
                {
                    Some(AtomicApi::ReadWrite)
                } else {
                    None
                };

                if let Some(api) = api {
                    log::debug!("Found atomic operation: {:?} in {}", api, fn_name);
                    // 获取atomic变量
                    if let Some(arg) = args.get(0) {
                        if let Operand::Move(first_place) | Operand::Copy(first_place) = &arg.node {
                            let var_name = format!(
                                "{}_{}",
                                self.tcx.def_path_str(self.instance.def_id()),
                                first_place.local.index()
                            );
                            let first_place_ty = &self.body.local_decls[first_place.local].ty;

                            log::debug!("Processing atomic variable: {}", var_name);

                            // 获取ordering参数
                            // 对于load, ordering参数在第二个位置
                            // 对于store, ordering参数在第三个位置
                            // 对于read-write, ordering参数在最后一个位置
                            let ordering_idx = match api {
                                AtomicApi::Read => 1,
                                AtomicApi::Write => 2,
                                AtomicApi::ReadWrite => args.len() - 1,
                            };

                            if let Some(arg) = args.get(ordering_idx) {
                                log::debug!("Found ordering argument: {:?}", arg);
                                match &arg.node {
                                    Operand::Constant(c) => {
                                        log::debug!("Found constant: {:?}", c);
                                        if let Some(val) = c.const_.try_to_scalar() {
                                            let ordering_val = val.to_u32().unwrap();
                                            log::debug!(
                                                "Found ordering value: {:?}",
                                                AtomicOrdering::from_u32(ordering_val)
                                            );
                                            if let Some(info) = self.atomic_vars.get_mut(&var_name)
                                            {
                                                log::debug!(
                                                    "Found ordering value: {}",
                                                    ordering_val
                                                );
                                                let op = AtomicOperation {
                                                    api,
                                                    ordering: AtomicOrdering::from_u32(
                                                        ordering_val,
                                                    ),
                                                    location: format!(
                                                        "{:?}",
                                                        self.body.source_info(location).span
                                                    ),
                                                };
                                                info.operations.push(op.clone());
                                                log::debug!("Added operation: {:?}", op);
                                            }
                                        }
                                    }
                                    Operand::Move(ordering_place) => {
                                        log::debug!("Found move operand: {:?}", ordering_place);
                                        let local_decl =
                                            &self.body.local_decls[ordering_place.local].ty;
                                        if local_decl.is_enum() {
                                            log::debug!("Found enum type: {:?}", local_decl);
                                            // 假设枚举值的顺序与 AtomicOrdering 匹配
                                            let ordering = match local_decl
                                                .discriminant_for_variant(
                                                    self.tcx,
                                                    VariantIdx::from_u32(0),
                                                ) {
                                                Some(discr) => AtomicOrdering::from_u128(discr.val),
                                                None => AtomicOrdering::SeqCst, // 默认值
                                            };
                                            log::debug!("Found ordering: {:?}", ordering);
                                            if let Some(info) = self.atomic_vars.get_mut(&var_name)
                                            {
                                                let op = AtomicOperation {
                                                    api,
                                                    ordering,
                                                    location: format!(
                                                        "{:?}",
                                                        self.body.source_info(location).span
                                                    ),
                                                };
                                                info.operations.push(op);
                                            } else {
                                                let op = AtomicOperation {
                                                    api,
                                                    ordering,
                                                    location: format!(
                                                        "{:?}",
                                                        self.body.source_info(location).span
                                                    ),
                                                };
                                                self.atomic_vars.insert(
                                                    var_name,
                                                    AtomicVarInfo {
                                                        var_type: first_place_ty.to_string(),
                                                        span: format!(
                                                            "{:?}",
                                                            self.body.source_info(location).span
                                                        ),
                                                        operations: vec![op],
                                                    },
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        log::error!("Unknown operand: {:?}", arg);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.super_terminator(terminator, location);
    }
}
