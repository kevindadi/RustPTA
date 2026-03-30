//! **CIR** (Concurrency Intermediate Representation): YAML artifact parallel to the Petri-net translation.
//!
//! - **MIR 路径**（推荐）: [`crate::translate::mir_to_cir::BodyToCir`] — 仅遍历 MIR、按 `TransitionType` 标签发射 CIR，**不**从 Petri 网拓扑推断顺序。
//! - **网路径**: [`pipeline::build_cir_from_petri_net`] — 合并 [`net_extract::CirExtractor`] 与 MIR 调用边。
//! - **Resource naming** matches pointer-analysis ids via [`resource_table::ResourceTable`] (Mutex/RwLock/CondVar/Atomic).
//! - **Optional**: extract sync-only structure from a bare [`crate::net::Net`] with [`CirExtractor::extract`].

pub mod diff;
pub mod function_grouper;
pub(crate) mod mir_emitter;
pub mod naming;
pub mod net_extract;
pub(crate) mod op_mapping;
pub mod pipeline;
pub mod protection;
pub mod resource_table;
pub mod types;
pub mod yaml;

pub use diff::CirDiff;
pub use net_extract::{CirExtractor, ExtractionError};
pub use pipeline::{build_cir_artifact_from_mir_emission, build_cir_from_petri_net, def_in_scope};
pub use types::CirArtifact;

#[cfg(test)]
mod tests;
