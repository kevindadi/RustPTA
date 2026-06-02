extern crate rustc_hir;
extern crate rustc_middle;

use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;

use rustc_middle::ty::{GenericArg, List};

pub fn is_arc_or_rc_clone<'tcx>(
    def_id: DefId,
    substs: &List<GenericArg<'tcx>>,
    tcx: TyCtxt<'tcx>,
) -> bool {
    let fn_name = tcx.def_path_str(def_id);
    if fn_name != "std::clone::Clone::clone" {
        return false;
    }
    if let &[arg] = substs.as_ref() {
        let arg_ty_name = format!("{:?}", arg);
        if is_arc(&arg_ty_name) || is_rc(&arg_ty_name) {
            return true;
        }
    }
    false
}

#[inline]
pub fn is_arc(arg_ty_name: &str) -> bool {
    arg_ty_name.starts_with("std::sync::Arc<")
}

#[inline]
pub fn is_rc(arg_ty_name: &str) -> bool {
    arg_ty_name.starts_with("std::rc::Rc<")
}

#[inline]
pub fn is_ptr_read(def_id: DefId, tcx: TyCtxt<'_>) -> bool {
    tcx.def_path_str(def_id).starts_with("std::ptr::read::<")
}

#[inline]
pub fn is_index(def_id: DefId, tcx: TyCtxt<'_>) -> bool {
    tcx.def_path_str(def_id).ends_with("::index")
}

/// Lock-acquiring methods whose returned guard conceptually refers to the
/// receiver lock object (`Mutex::lock`/`try_lock`, `RwLock::read|write` and
/// their `try_*` forms, across std / parking_lot / spin). Async variants are
/// intentionally excluded — they belong to the async engine.
#[inline]
pub fn is_lock_acquire(def_id: DefId, tcx: TyCtxt<'_>) -> bool {
    let path = tcx.def_path_str(def_id);
    if path.contains("tokio")
        || path.contains("futures")
        || path.contains("loom")
        || path.contains("async")
    {
        return false;
    }
    let method = path.rsplit("::").next().unwrap_or("");
    let mutex_lock = path.contains("Mutex") && matches!(method, "lock" | "try_lock");
    let rwlock_lock =
        path.contains("RwLock") && matches!(method, "read" | "write" | "try_read" | "try_write");
    mutex_lock || rwlock_lock
}

/// `Result`/`Option` extractors commonly chained after `lock()`
/// (`unwrap`/`expect`/`ok`/...). Used only to trace a lock guard back to its
/// acquiring call's receiver; this is *not* a points-to model, so it does not
/// affect analysis soundness elsewhere.
#[inline]
pub fn is_wrapper_extract(def_id: DefId, tcx: TyCtxt<'_>) -> bool {
    let path = tcx.def_path_str(def_id);
    if !(path.contains("Result") || path.contains("Option")) {
        return false;
    }
    let method = path.rsplit("::").next().unwrap_or("");
    matches!(
        method,
        "unwrap"
            | "expect"
            | "ok"
            | "unwrap_or"
            | "unwrap_or_else"
            | "unwrap_or_default"
            | "unwrap_unchecked"
    )
}
