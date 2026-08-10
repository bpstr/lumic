use crate::{ProcessRunner, ProcessSpec, hex_encode, resource_lock::ResourceLock};
use lumic_core::{LumicError, Result, artifact::ArtifactDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactInspection {
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEnsureResult {
    pub artifact: ArtifactInspection,
    pub downloaded: bool,
}

/// Downloads reviewed immutable artifacts into Lumic's private verified cache.
#[derive(Debug, Clone)]
pub struct ArtifactManager {
    state_dir: PathBuf,
    runner: ProcessRunner,
}

impl ArtifactManager {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            state_dir: state_dir.as_ref().to_path_buf(),
            runner: ProcessRunner,
        }
    }

    pub fn cached_path(&self, definition: &ArtifactDefinition, extension: &str) -> Result<PathBuf> {
        Ok(self
            .state_dir
            .join("artifacts")
            .join(definition.cache_file_name(extension)?))
    }

    pub fn inspect(
        &self,
        definition: &ArtifactDefinition,
        extension: &str,
    ) -> Result<Option<ArtifactInspection>> {
        definition.validate()?;
        let path = self.cached_path(definition, extension)?;
        if path.is_symlink() {
            return Err(invalid(
                "artifact_cache",
                "refusing a symbolic link in the artifact cache",
            ));
        }
        if !path.exists() {
            return Ok(None);
        }
        if !path.is_file() {
            return Err(invalid(
                "artifact_cache",
                "cached artifact must be a regular file",
            ));
        }
        let sha256 = sha256(&path)?;
        let size_bytes = fs::metadata(&path).map_err(io)?.len();
        Ok(Some(ArtifactInspection {
            verified: sha256 == definition.sha256,
            path,
            sha256,
            size_bytes,
        }))
    }

    pub async fn ensure(
        &self,
        definition: &ArtifactDefinition,
        extension: &str,
    ) -> Result<ArtifactEnsureResult> {
        definition.validate()?;
        let _lock = ResourceLock::try_acquire_artifact(
            &self.state_dir,
            &definition.id,
            &definition.version,
        )?;
        if let Some(inspection) = self.inspect(definition, extension)? {
            if inspection.verified {
                return Ok(ArtifactEnsureResult {
                    artifact: inspection,
                    downloaded: false,
                });
            }
            return Err(invalid(
                "artifact_cache",
                "cached artifact checksum does not match the pinned digest",
            ));
        }

        let path = self.cached_path(definition, extension)?;
        let directory = path
            .parent()
            .ok_or_else(|| invalid("artifact_cache", "artifact path has no parent"))?;
        fs::create_dir_all(directory).map_err(io)?;
        #[cfg(unix)]
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(io)?;
        let temporary = directory.join(format!(
            ".{}-{}-{}.download",
            definition.id,
            definition.version,
            std::process::id()
        ));
        if temporary.symlink_metadata().is_ok() {
            return Err(invalid(
                "artifact_cache",
                "temporary artifact path already exists",
            ));
        }
        let download = ProcessSpec::new("curl").args([
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--output",
            temporary.to_string_lossy().as_ref(),
            definition.url.as_str(),
        ]);
        if let Err(error) = self.run_checked(download).await {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        #[cfg(unix)]
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).map_err(io)?;
        let actual = sha256(&temporary)?;
        if actual != definition.sha256 {
            let _ = fs::remove_file(&temporary);
            return Err(invalid(
                "artifact",
                "downloaded artifact checksum does not match the pinned digest",
            ));
        }
        File::open(&temporary).map_err(io)?.sync_all().map_err(io)?;
        fs::rename(&temporary, &path).map_err(io)?;
        let artifact = self.inspect(definition, extension)?.ok_or_else(|| {
            invalid(
                "artifact_cache",
                "verified artifact disappeared during cache commit",
            )
        })?;
        Ok(ArtifactEnsureResult {
            artifact,
            downloaded: true,
        })
    }

    async fn run_checked(&self, mut spec: ProcessSpec) -> Result<()> {
        spec.timeout = Duration::from_secs(300);
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(())
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex_encode(&digest.finalize()))
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("artifact cache I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn definition() -> ArtifactDefinition {
        ArtifactDefinition {
            id: "wordpress".into(),
            version: "6.8.2".into(),
            url: "https://example.com/wordpress.tar.gz".into(),
            sha256: "576473d6a73d7f55ed882786af3060c6979bb16fe59962549cce647efe8f9f3f".into(),
        }
    }

    #[tokio::test]
    async fn ensure_reuses_only_a_verified_cached_artifact() {
        let root = std::env::temp_dir().join(format!(
            "lumic-artifact-cache-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let manager = ArtifactManager::at_state_dir(&root);
        let path = manager.cached_path(&definition(), "tar.gz").unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(&path)
            .unwrap()
            .write_all(b"wordpress")
            .unwrap();
        let result = manager.ensure(&definition(), "tar.gz").await.unwrap();
        assert!(!result.downloaded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inspect_reports_tampered_cache_entries_as_unverified() {
        let root = std::env::temp_dir().join(format!(
            "lumic-artifact-tamper-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let manager = ArtifactManager::at_state_dir(&root);
        let path = manager.cached_path(&definition(), "tar.gz").unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(&path).unwrap().write_all(b"tampered").unwrap();
        let inspection = manager.inspect(&definition(), "tar.gz").unwrap().unwrap();
        assert!(!inspection.verified);
        fs::remove_dir_all(root).unwrap();
    }
}
