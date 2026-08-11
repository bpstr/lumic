//! Cross-process advisory locks for resource reconciliation.

use lumic_core::{LumicError, Result, resource::ResourceRef};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ResourceLock {
    file: File,
    path: PathBuf,
}

impl ResourceLock {
    /// Takes an exclusive, non-blocking advisory lock for one resource.
    pub fn try_acquire(state_dir: impl AsRef<Path>, resource: &ResourceRef) -> Result<Self> {
        resource.validate()?;
        Self::try_acquire_key(state_dir, &resource.key().replace(':', "-"))
    }

    /// Takes the global package/repository mutation lock.
    pub fn try_acquire_packages(state_dir: impl AsRef<Path>) -> Result<Self> {
        Self::try_acquire_key(state_dir, "global-packages")
    }

    /// Takes the global nginx configuration/reload lock.
    pub fn try_acquire_nginx(state_dir: impl AsRef<Path>) -> Result<Self> {
        Self::try_acquire_key(state_dir, "global-nginx")
    }

    /// Takes a lock for one immutable artifact cache entry.
    pub fn try_acquire_artifact(
        state_dir: impl AsRef<Path>,
        artifact_id: &str,
        version: &str,
    ) -> Result<Self> {
        Self::try_acquire_key(state_dir, &format!("artifact-{artifact_id}-{version}"))
    }

    /// Takes a lock for one repository mutation.
    pub fn try_acquire_repository(
        state_dir: impl AsRef<Path>,
        namespace: &str,
        name: &str,
    ) -> Result<Self> {
        let digest = Sha256::digest(format!("{namespace}\0{name}").as_bytes());
        Self::try_acquire_key(state_dir, &format!("repository-{digest:x}"))
    }

    /// Serializes read-modify-write updates to the shared repository registry.
    pub fn acquire_repository_state(state_dir: impl AsRef<Path>) -> Result<Self> {
        Self::acquire_key(state_dir, "repository-state", false)
    }

    /// Serializes read-modify-write updates to application and deployment state.
    pub fn acquire_application_state(state_dir: impl AsRef<Path>) -> Result<Self> {
        Self::acquire_key(state_dir, "application-state", false)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn try_acquire_key(state_dir: impl AsRef<Path>, key: &str) -> Result<Self> {
        Self::acquire_key(state_dir, key, true)
    }

    fn acquire_key(state_dir: impl AsRef<Path>, key: &str, nonblocking: bool) -> Result<Self> {
        if !valid_key(key) {
            return Err(LumicError::InvalidInput {
                field: "lock.key".into(),
                message: "lock key contains unsupported characters".into(),
            });
        }
        let directory = state_dir.as_ref().join("locks");
        fs::create_dir_all(&directory).map_err(lock_io)?;
        let path = directory.join(format!("{key}.lock"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&path).map_err(lock_io)?;
        if !file.metadata().map_err(lock_io)?.is_file() {
            return Err(LumicError::InvalidInput {
                field: "lock.path".into(),
                message: "lock path must be a regular file".into(),
            });
        }
        // SAFETY: flock only reads the valid descriptor and does not retain a pointer.
        let operation = if nonblocking {
            libc::LOCK_EX | libc::LOCK_NB
        } else {
            libc::LOCK_EX
        };
        let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err(LumicError::Internal {
                    message: format!("resource lock '{}' is already held", path.display()),
                });
            }
            return Err(lock_io(error));
        }
        Ok(Self { file, path })
    }
}

impl Drop for ResourceLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains open until after this method returns.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn lock_io(error: impl std::fmt::Display) -> LumicError {
    LumicError::Internal {
        message: format!("resource lock failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::resource::ResourceKind;

    #[test]
    fn lock_is_exclusive_until_guard_drops() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-resource-lock-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let resource = ResourceRef::new(ResourceKind::ManagedService, "redis.main").unwrap();
        let first = ResourceLock::try_acquire(&directory, &resource).unwrap();
        assert!(ResourceLock::try_acquire(&directory, &resource).is_err());
        drop(first);
        assert!(ResourceLock::try_acquire(&directory, &resource).is_ok());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repository_lock_keys_accept_every_valid_repository_identity() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-repository-lock-key-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let lock = ResourceLock::try_acquire_repository(&directory, "Team", "API").unwrap();
        assert!(
            lock.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("repository-")
        );
        drop(lock);
        fs::remove_dir_all(directory).unwrap();
    }
}
