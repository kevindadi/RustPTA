//! Infer `protection: var -> [locks]` from sequential CIR statements.
//!
//! Previously used `read`/`write` ops on `Var` resources (from unsafe Petri transitions).
//! Those ops are no longer part of CIR YAML; this therefore returns an empty map.

use std::collections::BTreeMap;

use crate::cir::types::CirStatement;

pub fn infer_protection(
    _body: &[CirStatement],
    _resources: &BTreeMap<String, crate::cir::types::CirResource>,
) -> BTreeMap<String, Vec<String>> {
    BTreeMap::new()
}
