//! Verified, host-native installation support for repository-compatible Git forges.

use crate::{
    ProcessRunner, ProcessSpec, apt::AptPackageManager, artifact::ArtifactManager,
    event_store::EventStore,
};
use lumic_core::{
    LumicError, OperationContext, Result, artifact::ArtifactDefinition, package::PackageName,
    repository::validate_absolute_path,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const SHARED_GROUP: &str = "lumic-git";
const PREREQUISITES: &[&str] = &["ca-certificates", "curl", "git", "tar"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeArchive {
    Binary,
    TarGz { binary: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgeArtifact {
    pub architecture: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitForgeSpec {
    pub id: &'static str,
    pub version: &'static str,
    pub user: &'static str,
    pub binary_path: &'static str,
    pub data_path: &'static str,
    pub archive: ForgeArchive,
    pub artifacts: &'static [ForgeArtifact],
}

impl GitForgeSpec {
    pub fn artifact(&self) -> Result<ArtifactDefinition> {
        self.artifact_for(std::env::consts::ARCH)
    }

    pub fn artifact_for(&self, architecture: &str) -> Result<ArtifactDefinition> {
        let artifact = self
            .artifacts
            .iter()
            .find(|artifact| artifact.architecture == architecture)
            .ok_or_else(|| invalid("architecture", "forge artifacts support x86_64 and aarch64"))?;
        Ok(ArtifactDefinition {
            id: format!("{}-linux-{}", self.id, architecture.replace('_', "-")),
            version: self.version.into(),
            url: artifact.url.into(),
            sha256: artifact.sha256.into(),
        })
    }

    pub const fn extension(&self) -> &'static str {
        match self.archive {
            ForgeArchive::Binary => "bin",
            ForgeArchive::TarGz { .. } => "tar.gz",
        }
    }
}

pub static GITEA_SPEC: GitForgeSpec = GitForgeSpec {
    id: "gitea",
    version: "1.27.1",
    user: "gitea",
    binary_path: "/usr/local/bin/gitea",
    data_path: "/var/lib/gitea",
    archive: ForgeArchive::Binary,
    artifacts: &[
        ForgeArtifact {
            architecture: "x86_64",
            url: "https://github.com/go-gitea/gitea/releases/download/v1.27.1/gitea-1.27.1-linux-amd64",
            sha256: "86a7ac26e7f9c9cca0f56c4fac07fff205d5fc3bca0e54af23a204f07b833bc9",
        },
        ForgeArtifact {
            architecture: "aarch64",
            url: "https://github.com/go-gitea/gitea/releases/download/v1.27.1/gitea-1.27.1-linux-arm64",
            sha256: "aa544be7d305ddc7a0fac389e562b698a2b9d5059314177257c4daf08cf38827",
        },
    ],
};

pub static GOGS_SPEC: GitForgeSpec = GitForgeSpec {
    id: "gogs",
    version: "0.14.3",
    user: "gogs",
    binary_path: "/usr/local/bin/gogs",
    data_path: "/var/lib/gogs",
    archive: ForgeArchive::TarGz {
        binary: "gogs/gogs",
    },
    artifacts: &[
        ForgeArtifact {
            architecture: "x86_64",
            url: "https://github.com/gogs/gogs/releases/download/v0.14.3/gogs_v0.14.3_linux_amd64.tar.gz",
            sha256: "c27fbd8337ebd661929389f5237bf601e09958d514835c99fad3b904c63bedb2",
        },
        ForgeArtifact {
            architecture: "aarch64",
            url: "https://github.com/gogs/gogs/releases/download/v0.14.3/gogs_v0.14.3_linux_arm64.tar.gz",
            sha256: "8cd4659144900235701b96132b9893d5f993a410eeb98d9b904ccb1011451d40",
        },
    ],
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeInstallResult {
    pub changed: bool,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct GitForgeInstaller {
    state_dir: PathBuf,
    artifacts: ArtifactManager,
    packages: AptPackageManager,
    runner: ProcessRunner,
}

impl GitForgeInstaller {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        Self {
            artifacts: ArtifactManager::at_state_dir(&state_dir),
            packages: AptPackageManager::system(EventStore::at_state_dir(&state_dir)),
            runner: ProcessRunner,
            state_dir,
        }
    }

    pub async fn install(
        &self,
        spec: &GitForgeSpec,
        repository_root: &Path,
        context: &OperationContext,
    ) -> Result<ForgeInstallResult> {
        validate_absolute_path("git.repository_root", repository_root)?;
        spec.artifact()?.validate()?;
        if context.dry_run {
            return Ok(ForgeInstallResult {
                changed: false,
                version: spec.version.into(),
            });
        }
        let mut changed = false;
        for package in PREREQUISITES {
            changed |= self
                .packages
                .install(&PackageName::parse(*package)?, context)
                .await?
                .changed;
        }
        changed |= self.prepare_account(spec, repository_root).await?;
        let definition = spec.artifact()?;
        let artifact = self.artifacts.ensure(&definition, spec.extension()).await?;
        changed |= artifact.downloaded;
        let source = self.binary_source(spec, &artifact.artifact.path).await?;
        let target = PathBuf::from(spec.binary_path);
        let digest_target = target.clone();
        let digest_source = source.clone();
        let binary_changed = tokio::task::spawn_blocking(move || -> Result<bool> {
            Ok(!digest_target.is_file() || digest(&digest_target)? != digest(&digest_source)?)
        })
        .await
        .map_err(|error| LumicError::Internal {
            message: format!("forge checksum task failed: {error}"),
        })??;
        if binary_changed {
            self.run_checked(ProcessSpec::new("install").args([
                "--mode=0755",
                "--owner=root",
                "--group=root",
                source.to_string_lossy().as_ref(),
                spec.binary_path,
            ]))
            .await?;
            changed = true;
        }
        self.clean_staging(spec)?;
        Ok(ForgeInstallResult {
            changed,
            version: spec.version.into(),
        })
    }

    pub async fn inspect_version(&self, spec: &GitForgeSpec) -> Result<Option<String>> {
        let binary = Path::new(spec.binary_path);
        if !binary.is_file() || binary.is_symlink() {
            return Ok(None);
        }
        let output = self
            .run_checked(ProcessSpec::new(spec.binary_path).args(["--version"]))
            .await?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(Some(
            text.split_whitespace()
                .find(|part| {
                    part.chars()
                        .next()
                        .is_some_and(|value| value.is_ascii_digit())
                })
                .unwrap_or(spec.version)
                .trim_start_matches('v')
                .to_owned(),
        ))
    }

    pub fn remove_binary(&self, spec: &GitForgeSpec, context: &OperationContext) -> Result<bool> {
        let target = Path::new(spec.binary_path);
        if !target.exists() {
            return Ok(false);
        }
        if !target.is_file() || target.is_symlink() {
            return Err(invalid(
                "forge.binary",
                "refusing to remove a non-regular binary",
            ));
        }
        if context.dry_run {
            return Ok(false);
        }
        fs::remove_file(target).map_err(io)?;
        Ok(true)
    }

    async fn prepare_account(&self, spec: &GitForgeSpec, repository_root: &Path) -> Result<bool> {
        let mut changed = false;
        if !self
            .command_succeeds(ProcessSpec::new("getent").args(["group", SHARED_GROUP]))
            .await?
        {
            self.run_checked(ProcessSpec::new("groupadd").args(["--system", SHARED_GROUP]))
                .await?;
            changed = true;
        }
        if !self
            .command_succeeds(ProcessSpec::new("id").args(["--user", spec.user]))
            .await?
        {
            self.run_checked(ProcessSpec::new("useradd").args([
                "--system",
                "--gid",
                SHARED_GROUP,
                "--home-dir",
                spec.data_path,
                "--shell",
                "/usr/sbin/nologin",
                spec.user,
            ]))
            .await?;
            changed = true;
        }
        self.run_checked(ProcessSpec::new("install").args([
            "--directory",
            "--mode=2770",
            "--owner=root",
            &format!("--group={SHARED_GROUP}"),
            repository_root.to_string_lossy().as_ref(),
        ]))
        .await?;
        self.run_checked(ProcessSpec::new("chgrp").args([
            "-R",
            SHARED_GROUP,
            repository_root.to_string_lossy().as_ref(),
        ]))
        .await?;
        self.run_checked(ProcessSpec::new("chmod").args([
            "-R",
            "g+rwX",
            repository_root.to_string_lossy().as_ref(),
        ]))
        .await?;
        self.run_checked(ProcessSpec::new("install").args([
            "--directory",
            "--mode=0750",
            &format!("--owner={}", spec.user),
            &format!("--group={SHARED_GROUP}"),
            spec.data_path,
        ]))
        .await?;
        Ok(changed)
    }

    async fn binary_source(&self, spec: &GitForgeSpec, artifact: &Path) -> Result<PathBuf> {
        match spec.archive {
            ForgeArchive::Binary => Ok(artifact.to_path_buf()),
            ForgeArchive::TarGz { binary } => {
                let staging = self.staging_path(spec);
                if staging.exists() {
                    return Err(invalid("forge.staging", "staging directory already exists"));
                }
                fs::create_dir_all(&staging).map_err(io)?;
                self.run_checked(ProcessSpec::new("tar").args([
                    "--extract",
                    "--gzip",
                    "--file",
                    artifact.to_string_lossy().as_ref(),
                    "--directory",
                    staging.to_string_lossy().as_ref(),
                    "--no-same-owner",
                    "--no-same-permissions",
                    binary,
                ]))
                .await?;
                let source = staging.join(binary);
                if !source.is_file() || source.is_symlink() {
                    self.clean_staging(spec)?;
                    return Err(invalid(
                        "forge.artifact",
                        "archive does not contain a regular binary",
                    ));
                }
                Ok(source)
            }
        }
    }

    fn staging_path(&self, spec: &GitForgeSpec) -> PathBuf {
        self.state_dir.join("forge-staging").join(format!(
            "{}-{}-{}",
            spec.id,
            spec.version,
            std::process::id()
        ))
    }

    fn clean_staging(&self, spec: &GitForgeSpec) -> Result<()> {
        let staging = self.staging_path(spec);
        if staging.exists() {
            fs::remove_dir_all(staging).map_err(io)?;
        }
        Ok(())
    }

    async fn command_succeeds(&self, spec: ProcessSpec) -> Result<bool> {
        Ok(self.runner.run(&spec).await?.success())
    }

    async fn run_checked(&self, mut spec: ProcessSpec) -> Result<crate::ProcessOutput> {
        spec.timeout = Duration::from_secs(300);
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("Git forge installation I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_artifacts_are_valid_for_supported_architectures() {
        for spec in [&GITEA_SPEC, &GOGS_SPEC] {
            for architecture in ["x86_64", "aarch64"] {
                spec.artifact_for(architecture).unwrap().validate().unwrap();
            }
            assert!(spec.artifact_for("powerpc").is_err());
        }
    }
}
