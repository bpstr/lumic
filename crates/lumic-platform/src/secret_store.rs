use crate::{atomic_file::write_atomic, hex_encode};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use lumic_core::{LumicError, Result, managed_service::validate_resource_id};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const ENVELOPE_MAGIC: &[u8] = b"LUMICSEC1";
const MASTER_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Debug, Clone)]
pub struct SecretStore {
    directory: PathBuf,
    key_path: PathBuf,
}

impl SecretStore {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            directory: state_dir.as_ref().join("secrets"),
            key_path: state_dir.as_ref().join("secrets.key"),
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
        let value = hex_encode(&random).into_bytes();
        self.write_encrypted(reference, &path, &value)?;
        Ok(reference.to_owned())
    }

    pub fn rotate(&self, reference: &str) -> Result<String> {
        validate_resource_id("secret_reference", reference)?;
        if !self.exists(reference)? {
            return Err(LumicError::InvalidInput {
                field: "secret_reference".into(),
                message: "secret reference does not exist".into(),
            });
        }
        let mut random = [0_u8; 32];
        fill_random(&mut random)?;
        self.put(reference, hex_encode(&random).as_bytes())
    }

    pub fn put(&self, reference: &str, value: &[u8]) -> Result<String> {
        validate_resource_id("secret_reference", reference)?;
        if value.is_empty() || value.len() > 16 * 1024 || value.contains(&0) {
            return Err(LumicError::InvalidInput {
                field: "secret".into(),
                message: "must be non-empty, at most 16 KiB, and contain no NUL bytes".into(),
            });
        }
        self.write_encrypted(reference, &self.path(reference)?, value)?;
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
        let bytes = fs::read(&path).map_err(secret_io)?;
        if !bytes.starts_with(ENVELOPE_MAGIC) {
            // Existing installations stored private files without encryption. Migrate
            // them on first use while returning the same value to the caller.
            self.put(reference, &bytes)?;
            return Ok(bytes);
        }
        self.decrypt(reference, &bytes)
    }

    pub fn delete(&self, reference: &str) -> Result<()> {
        let path = self.path(reference)?;
        if path.exists() {
            fs::remove_file(&path).map_err(secret_io)?;
        }
        let backup = secret_backup_path(&path)?;
        if backup.exists() {
            fs::remove_file(backup).map_err(secret_io)?;
        }
        Ok(())
    }

    fn path(&self, reference: &str) -> Result<PathBuf> {
        validate_resource_id("secret_reference", reference)?;
        Ok(self.directory.join(reference))
    }

    fn write_encrypted(&self, reference: &str, path: &Path, value: &[u8]) -> Result<()> {
        let key = self.master_key()?;
        let mut nonce = [0_u8; NONCE_BYTES];
        fill_random(&mut nonce)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: value,
                    aad: reference.as_bytes(),
                },
            )
            .map_err(|_| secret_crypto("could not encrypt secret value"))?;
        let mut envelope =
            Vec::with_capacity(ENVELOPE_MAGIC.len() + NONCE_BYTES + ciphertext.len());
        envelope.extend_from_slice(ENVELOPE_MAGIC);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        write_atomic(path, &envelope, 0o600)?;
        Ok(())
    }

    fn decrypt(&self, reference: &str, envelope: &[u8]) -> Result<Vec<u8>> {
        let nonce_start = ENVELOPE_MAGIC.len();
        let ciphertext_start = nonce_start + NONCE_BYTES;
        if envelope.len() <= ciphertext_start {
            return Err(secret_crypto("encrypted secret envelope is truncated"));
        }
        let key = self.master_key()?;
        ChaCha20Poly1305::new(Key::from_slice(&key))
            .decrypt(
                Nonce::from_slice(&envelope[nonce_start..ciphertext_start]),
                Payload {
                    msg: &envelope[ciphertext_start..],
                    aad: reference.as_bytes(),
                },
            )
            .map_err(|_| secret_crypto("encrypted secret authentication failed"))
    }

    fn master_key(&self) -> Result<[u8; MASTER_KEY_BYTES]> {
        if self.key_path.exists() {
            return read_master_key(&self.key_path);
        }
        if let Some(parent) = self.key_path.parent() {
            fs::create_dir_all(parent).map_err(secret_io)?;
        }
        let mut key = [0_u8; MASTER_KEY_BYTES];
        fill_random(&mut key)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&self.key_path) {
            Ok(mut file) => {
                file.write_all(&key).map_err(secret_io)?;
                file.sync_all().map_err(secret_io)?;
                Ok(key)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                read_master_key(&self.key_path)
            }
            Err(error) => Err(secret_io(error)),
        }
    }
}

fn secret_backup_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| secret_crypto("secret path has no parent"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| secret_crypto("secret path has no UTF-8 file name"))?;
    Ok(parent.join(format!(".{name}.lumic-backup")))
}

fn read_master_key(path: &Path) -> Result<[u8; MASTER_KEY_BYTES]> {
    let metadata = fs::symlink_metadata(path).map_err(secret_io)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(secret_crypto("master key is not a regular file"));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(secret_crypto("master key permissions must be 0600"));
    }
    let bytes = fs::read(path).map_err(secret_io)?;
    bytes
        .try_into()
        .map_err(|_| secret_crypto("master key must contain exactly 32 bytes"))
}

fn fill_random(bytes: &mut [u8]) -> Result<()> {
    let mut source = fs::File::open("/dev/urandom").map_err(secret_io)?;
    source.read_exact(bytes).map_err(secret_io)
}

fn secret_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("secret store I/O failed: {error}"),
    }
}

fn secret_crypto(message: &str) -> LumicError {
    LumicError::Internal {
        message: format!("secret store cryptography failed: {message}"),
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
        let encrypted = fs::read(directory.join("secrets/db-user-password")).unwrap();
        assert!(encrypted.starts_with(ENVELOPE_MAGIC));
        assert_ne!(encrypted.len(), 64);
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

    #[test]
    fn encrypts_values_and_authenticates_the_reference() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-secret-encrypted-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = SecretStore::at_state_dir(&directory);
        store
            .put("application-demo-token", b"clear-text-value")
            .unwrap();
        let stored = fs::read(directory.join("secrets/application-demo-token")).unwrap();
        assert!(
            !stored
                .windows(b"clear-text-value".len())
                .any(|value| value == b"clear-text-value")
        );
        assert_eq!(
            store.read("application-demo-token").unwrap(),
            b"clear-text-value"
        );
        fs::copy(
            directory.join("secrets/application-demo-token"),
            directory.join("secrets/application-other-token"),
        )
        .unwrap();
        assert!(store.read("application-other-token").is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migrates_legacy_private_files_when_they_are_read() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-secret-legacy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let secret_directory = directory.join("secrets");
        fs::create_dir_all(&secret_directory).unwrap();
        write_atomic(
            &secret_directory.join("legacy-token"),
            b"legacy-value",
            0o600,
        )
        .unwrap();
        let store = SecretStore::at_state_dir(&directory);
        assert_eq!(store.read("legacy-token").unwrap(), b"legacy-value");
        assert!(
            fs::read(secret_directory.join("legacy-token"))
                .unwrap()
                .starts_with(ENVELOPE_MAGIC)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn delete_removes_the_current_and_rotated_ciphertexts() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-secret-delete-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = SecretStore::at_state_dir(&directory);
        store.put("application-demo-token", b"first-value").unwrap();
        store
            .put("application-demo-token", b"second-value")
            .unwrap();
        let path = directory.join("secrets/application-demo-token");
        let backup = secret_backup_path(&path).unwrap();
        assert!(path.is_file());
        assert!(backup.is_file());

        store.delete("application-demo-token").unwrap();

        assert!(!path.exists());
        assert!(!backup.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
