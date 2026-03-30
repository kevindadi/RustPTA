//! YAML serialization for [`CirArtifact`](crate::cir::types::CirArtifact).
use crate::cir::types::CirArtifact;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CirYamlError {
    #[error("serde_yaml: {0}")]
    Serde(#[from] serde_yaml::Error),
}

pub fn to_yaml(artifact: &CirArtifact) -> Result<String, CirYamlError> {
    Ok(serde_yaml::to_string(artifact)?)
}

pub fn from_yaml(s: &str) -> Result<CirArtifact, CirYamlError> {
    Ok(serde_yaml::from_str(s)?)
}

impl CirArtifact {
    pub fn to_yaml(&self) -> Result<String, CirYamlError> {
        to_yaml(self)
    }

    pub fn from_yaml(s: &str) -> Result<Self, CirYamlError> {
        from_yaml(s)
    }
}
