use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum FirmwareArtifactError {
    #[error("无法读取固件符号文件 {path}: {detail}")]
    Unavailable { path: String, detail: String },
}

/// Immutable firmware/debug-symbol artifact shared by embedded observation producers.
///
/// This boundary owns local file loading only. RTT symbols, variable symbols and trace metadata are
/// interpreted by their respective domain modules instead of turning this into a universal parser.
#[derive(Clone)]
pub struct FirmwareArtifact {
    path: PathBuf,
    bytes: Arc<[u8]>,
}

impl FirmwareArtifact {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FirmwareArtifactError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|error| FirmwareArtifactError::Unavailable {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            bytes: Arc::from(bytes),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
