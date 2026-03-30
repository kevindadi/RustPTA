use std::fs;
use std::io;
use std::path::Path;

use crate::cir::types::CirArtifact;

impl CirArtifact {
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    pub fn from_yaml(s: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(s)
    }

    pub fn to_yaml_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let yaml = self
            .to_yaml()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path.as_ref(), yaml)
    }

    pub fn from_yaml_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path.as_ref())?;
        Ok(Self::from_yaml(&content)?)
    }
}
