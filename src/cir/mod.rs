//! Petri net (`net::core::Net`) → native CIR extraction and diff.

pub mod diff;
pub mod extractor;
pub mod integration;
pub mod types;
pub mod yaml;

pub use diff::CirDiff;
pub use extractor::{CirExtractor, ExtractionError, RawFunction};
pub use integration::{extract_and_verify, CirVerificationResult};
pub use types::*;
