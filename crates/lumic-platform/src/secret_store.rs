use crate::atomic_file::write_atomic;
use lumic_core::{LumicError, Result, managed_service::validate_resource_id};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct SecretStore {
    directory: PathBuf,
}

impl SecretStore {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            directory: state_dir.as_ref().join("secrets"),
        }
    }

    pub fn create(&self, reference: &str) -> Result<String> {
        validate_resource_id("secret_reference", reference)?;
        let path = self.path(reference)?;
        if path.exists() {
            return Err(LumicError::InvalidInput {
                field: "secret_reference".into(),
                message: "secret reference already exists".into(),
            });
        }
        let mut random = [0_u8; 32];
        let mut source = fs::File::open("/dev/urandom").map_err(secret_io)?;
        std::io::Read::read_exact(&mut source, &mut random).map_err(secret_io)?;
        let value = hex(&random).into_bytes();
        write_atomic(&path, &value, 0o600)?;
        Ok(reference.to_owned())
    }

    pub fn put(&self, reference: &str, value: &[u8]) -> Result<String> {
        validate_resource_id("secret_reference", reference)?;
        if value.is_empty() || value.len() > 16 * 1024 || value.contains(&0) {
            return Err(LumicError::InvalidInput {
                field: "secret".into(),
                message: "must be non-empty, at most 16 KiB, and contain no NUL bytes".into(),
            });
        }
        write_atomic(&self.path(reference)?, value, 0o600)?;
        Ok(reference.to_owned())
    }

    pub fn exists(&self, reference: &str) -> Result<bool> {
        Ok(self.path(reference)?.is_file())
    }

    pub(crate) fn read(&self, reference: &str) -> Result<Vec<u8>> {
        let path = self.path(reference)?;
        let metadata = fs::symlink_metadata(&path).map_err(secret_io)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(LumicError::InvalidInput {
                field: "secret_reference".into(),
                message: "secret reference is not a regular file".into(),
            });
        }
        fs::read(path).map_err(secret_io)
    }

    pub fn delete(&self, reference: &str) -> Result<()> {
        let path = self.path(reference)?;
        if path.exists() {
            fs::remove_file(path).map_err(secret_io)?;
        }
        Ok(())
    }

    fn path(&self, reference: &str) -> Result<PathBuf> {
        validate_resource_id("secret_reference", reference)?;
        Ok(self.directory.join(reference))
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn secret_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("secret store I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn creates_private_random_secret_and_returns_only_a_reference_later() {
        let directory = std::env::temp_dir().join(format!("lumic-secret-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let store = SecretStore::at_state_dir(&directory);
        let reference = store.create("db-user-password").unwrap();
        assert_eq!(reference, "db-user-password");
        assert_eq!(store.read(&reference).unwrap().len(), 64);
        assert!(store.create("db-user-password").is_err());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(directory.join("secrets/db-user-password"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
