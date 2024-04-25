use std::rc::Rc;

use rustc_hir::def_id::DefId;
use rustc_middle::ty;
use rustc_middle::ty::TyCtxt;
#[macro_export]
macro_rules! unrecoverable {
    ($fmt:expr) => (
        panic!(concat!("unrecoverable: ", stringify!($fmt)));
    );
    ($fmt:expr, $($arg:tt)+) => (
        panic!(concat!("unrecoverable: ", stringify!($fmt)), $($arg)+);
    );
}

/// Constructs a name for the crate that contains the given def_id.
fn crate_name(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    tcx.crate_name(def_id.krate).as_str().to_string()
}

pub fn def_id_as_qualified_name_str(tcx: TyCtxt<'_>, def_id: DefId) -> Rc<str> {
    let mut name = format!("/{}/", crate_name(tcx, def_id));
    name.push_str(&tcx.def_path_str(def_id));
    if tcx.def_kind(def_id).is_fn_like() {
        let fn_ty = tcx.type_of(def_id).skip_binder();
        name.push('(');
        let fn_sig = if fn_ty.is_fn() {
            fn_ty.fn_sig(tcx).skip_binder()
        } else if let ty::Closure(_, args) = fn_ty.kind() {
            args.as_closure().sig().skip_binder()
        } else {
            unreachable!()
        };
        let mut first = true;
        for param_ty in fn_sig.inputs() {
            if first {
                first = false;
            } else {
                name.push(',')
            }
            name.push_str(&format!("{param_ty:?}"));
        }
        name.push_str(")->");
        name.push_str(&format!("{:?}", fn_sig.output()));
    }
    Rc::from(name.as_str())
}
