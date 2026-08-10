//! Cross-process advisory locks for resource reconciliation.

use lumic_core::{LumicError, Result, resource::ResourceRef};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn try_acquire_key(state_dir: impl AsRef<Path>, key: &str) -> Result<Self> {
        if !valid_key(key) {
            return Err(LumicError::InvalidInput {
                field: "lock.key".into(),
                message: "lock key contains unsupported characters".into(),
            });
        }
        let directory = state_dir.as_ref().join("locks");
        fs::create_dir_all(&directory).map_err(lock_io)?;
        let path = directory.join(format!("{key}.lock"));
        if path.is_symlink() {
            return Err(LumicError::InvalidInput {
                field: "lock.path".into(),
                message: "refusing a symbolic-link lock file".into(),
            });
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(lock_io)?;
        // SAFETY: flock only reads the valid descriptor and does not retain a pointer.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
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
}
