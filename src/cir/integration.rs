//! One-shot extract + optional conformance check vs expected artifact.

use crate::cir::diff::CirDiff;
use crate::cir::extractor::{CirExtractor, ExtractionError};
use crate::cir::types::CirArtifact;

#[derive(Debug, Clone)]
pub struct CirVerificationResult {
    pub extracted: Option<CirArtifact>,
    pub extraction_errors: Vec<ExtractionError>,
    pub diff: Option<CirDiff>,
    pub conformant: bool,
}

/// Run extractor; if `expected` is `Some`, compare structurally.
pub fn extract_and_verify(
    net: &crate::net::core::Net,
    expected: Option<&CirArtifact>,
) -> CirVerificationResult {
    let ext = CirExtractor::new(net).extract();
    match ext {
        Ok(artifact) => {
            let (diff, conformant) = if let Some(e) = expected {
                let d = CirDiff::compare(e, &artifact);
                let c = d.is_conformant();
                (Some(d), c)
            } else {
                (None, true)
            };
            CirVerificationResult {
                extracted: Some(artifact),
                extraction_errors: Vec::new(),
                diff,
                conformant,
            }
        }
        Err(errs) => CirVerificationResult {
            extracted: None,
            extraction_errors: errs,
            diff: None,
            conformant: false,
        },
    }
}
