pub mod mem_watcher;

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::rc::Rc;
use toml;

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

/// Dumps a human readable MIR redendering of the function with the given DefId to standard output.
pub fn pretty_print_mir(tcx: TyCtxt<'_>, def_id: DefId) {
    if !matches!(
        tcx.def_kind(def_id),
        rustc_hir::def::DefKind::Struct | rustc_hir::def::DefKind::Variant
    ) {
        let mut stdout = std::io::stdout();
        stdout.write_fmt(format_args!("{:?}", def_id)).unwrap();
        rustc_middle::mir::write_mir_pretty(tcx, Some(def_id), &mut stdout).unwrap();
        let _ = stdout.flush();
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiSpec {
    /// 单独的 API 路径列表
    pub apis: Vec<ApiEntry>,
}

impl Default for ApiSpec {
    fn default() -> Self {
        ApiSpec { apis: vec![] }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ApiEntry {
    /// 单个 API 路径
    Single(String),
    /// API 路径组（用于表示需要一起分析的 API）
    Group(Vec<String>),
}

impl ApiSpec {
    pub fn parse(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let spec: ApiSpec = toml::from_str(&content)?;
        Ok(spec)
    }

    /// 获取所有单独的 API 路径
    pub fn get_single_apis(&self) -> Vec<String> {
        self.apis
            .iter()
            .filter_map(|entry| match entry {
                ApiEntry::Single(api) => Some(api.clone()),
                _ => None,
            })
            .collect()
    }

    /// 获取所有 API 组
    pub fn get_api_groups(&self) -> Vec<Vec<String>> {
        self.apis
            .iter()
            .filter_map(|entry| match entry {
                ApiEntry::Group(apis) => Some(apis.clone()),
                _ => None,
            })
            .collect()
    }
}

// parse the api spec file
pub(crate) fn parse_api_spec(api_spec_path: &str) -> Result<ApiSpec, Box<dyn std::error::Error>> {
    ApiSpec::parse(api_spec_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_api_spec() {
        let api_spec = parse_api_spec("tests/lib.toml").unwrap();
        println!("{:?}", api_spec);
    }
}
