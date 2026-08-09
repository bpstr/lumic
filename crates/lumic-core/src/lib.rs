use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    pub os: String,
    pub architecture: String,
}

impl HostFacts {
    pub fn new(os: impl Into<String>, architecture: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            architecture: architecture.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum LumicError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("platform inspection failed: {0}")]
    Platform(String),
}

pub type Result<T> = std::result::Result<T, LumicError>;
