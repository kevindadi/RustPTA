//! Unified field-sensitive, k-CFA, inclusion-based (Andersen) pointer analysis.
//!
//! The engine core (`intern`, `loc`, `context`, `constraint`, `solver`, `result`)
//! is decoupled from rustc and works over interned integer ids, so it can be unit
//! tested in isolation. The rustc-facing layers (MIR `builder`, library
//! `model`s, and the `adapter` shim) are added in later phases.
//!
//! See `docs/superpowers/specs/2026-06-02-pointer-analysis-refactor-design.md`.

pub mod adapter;
pub mod analysis;
pub mod builder;
pub mod constraint;
pub mod context;
pub mod intern;
pub mod interproc;
pub mod loc;
pub mod model;
pub mod result;
pub mod solver;

pub use adapter::PtaAliasAnalysis;
pub use analysis::PointerAnalysis;
pub use builder::{assignment_edges, build_body, EdgeKind, Form, Kind, PendingCall};
pub use constraint::{Constraint, ConstraintSet};
pub use interproc::{bind_call_edges, FuncMap};
pub use context::{CallSite, Context, ContextPolicy, KCallSite};
pub use loc::{AbstractLoc, AllocSite, FieldPath, LocArena, LocId, ProjElem};
pub use model::{CallModel, CallNodes, ModelRegistry};
pub use result::PointsToResult;
pub use solver::{PointsTo, Solver};
