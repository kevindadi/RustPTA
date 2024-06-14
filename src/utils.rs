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
        panic!(concat!("unrecoverable: ", stringify!($fmt)), $($arg)+)
    );
}

/// Constructs a name for the crate that contains the given def_id.
fn crate_name(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    tcx.crate_name(def_id.krate).as_str().to_string()
}

/// Extracts a function name from the DefId of a function.
pub fn format_name(def_id: DefId) -> String {
    let tmp1 = format!("{def_id:?}");
    let tmp2: &str = tmp1.split("~ ").collect::<Vec<&str>>()[1];
    let tmp3 = tmp2.replace(')', "");
    let lhs = tmp3.split('[').collect::<Vec<&str>>()[0];
    let rhs = tmp3.split(']').collect::<Vec<&str>>()[1];
    format!("{lhs}{rhs}").to_string()
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

#[macro_export]
macro_rules! buchi{
    (
        $(
            $src: ident
                $([$( $ltl:expr ),*] => $dest: ident)*
        )*
        ===
        init = [$( $init:ident ),*]
        accepting = [$( $accepting_state:ident ),*]
    ) => {{
        let mut __graph = Buchi::new();
        $(
            let mut $src = BuchiNode::new(stringify!($src).to_string());
            $(
                $src.adj.push(
                    BuchiNode {
                        id: stringify!($dest).into(),
                        labels: vec![$($ltl),*],
                        adj: vec![],
                    }
                );
            )*

            __graph.adj_list.push($src.clone());
        )*

        $(__graph.init_states.push($init.clone());)*
        $(__graph.accepting_states.push($accepting_state.clone());)*

        __graph
    }};
}

#[macro_export]
macro_rules! gbuchi{
    (
        $(
            $src: ident
                $([$ltl:expr] => $dest: ident)*
        )*
        ===
        init = [$( $init:ident ),*]
        $(accepting = [$( $accepting_states:expr ),*])*
    ) => {{
        let mut __graph = GeneralBuchi::new();
        $(
            let mut $src = BuchiNode::new(stringify!($src).to_string());
            $(
                $src.adj.push(
                    BuchiNode {
                        id: stringify!($dest).into(),
                        labels: vec![$ltl],
                        adj: vec![],
                    }
                );
            )*

            __graph.adj_list.push($src.clone());
        )*

        $(__graph.init_states.push($init.clone());)*
        $($(__graph.accepting_states.push($accepting_states.clone());)*)*

        __graph
    }};
}

#[macro_export]
macro_rules! kripke{
    (
        $(
            $world:ident = [$( $prop:expr),*]
        )*
        ===
        $(
            $src:ident R $dst:ident
        )*
        ===
        init = [$( $init:ident ),*]
    ) => {{
        let mut __kripke = KripkeStructure::new(vec![]);

        $(
            let mut $world = World {
                id: stringify!($world).into(),
                assignement: std::collections::HashMap::new(),
            };
            $(
                $world.assignement.insert($prop.0.into(), $prop.1);
            )*

            __kripke.add_world($world.clone());
        )*

        $(
            __kripke.add_relation($src.clone(), $dst.clone());
        )*

        __kripke.inits = vec![$($init.id.clone(),)*];

        __kripke
    }};
}
