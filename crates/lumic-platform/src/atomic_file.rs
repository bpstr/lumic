use lumic_core::{LumicError, Result};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicWriteResult {
    pub changed: bool,
    pub backup: Option<PathBuf>,
}

pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<AtomicWriteResult> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(LumicError::InvalidInput {
            field: "path".into(),
            message: "configuration path must be absolute".into(),
        });
    }
    if path.is_symlink() {
        return Err(LumicError::InvalidInput {
            field: "path".into(),
            message: "refusing to replace a symbolic link".into(),
        });
    }
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(AtomicWriteResult {
            changed: false,
            backup: None,
        });
    }
    let parent = path
        .parent()
        .ok_or_else(|| io_error("path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(error.to_string()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io_error("path has no UTF-8 file name"))?;
    let temporary = parent.join(format!(".{name}.lumic-{}.tmp", std::process::id()));
    let backup = if path.is_file() {
        let backup = parent.join(format!(".{name}.lumic-backup"));
        fs::copy(path, &backup).map_err(|error| io_error(error.to_string()))?;
        Some(backup)
    } else {
        None
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode);
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|error| io_error(error.to_string()))?;
        file.write_all(contents)
            .map_err(|error| io_error(error.to_string()))?;
        file.sync_all()
            .map_err(|error| io_error(error.to_string()))?;
        fs::rename(&temporary, path).map_err(|error| io_error(error.to_string()))?;
        Ok(AtomicWriteResult {
            changed: true,
            backup,
        })
    })();
    if temporary.exists() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn restore_backup(path: &Path, backup: &Path) -> Result<()> {
    if !backup.is_file() || backup.parent() != path.parent() {
        return Err(LumicError::InvalidInput {
            field: "backup".into(),
            message: "backup must be a regular sibling file".into(),
        });
    }
    fs::rename(backup, path).map_err(|error| io_error(error.to_string()))
}

fn io_error(message: impl Into<String>) -> LumicError {
    LumicError::Internal {
        message: format!("atomic configuration write failed: {}", message.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_idempotently_and_keeps_recoverable_previous_content() {
        let directory = std::env::temp_dir().join(format!("lumic-atomic-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("service.conf");
        let path = path.canonicalize().unwrap_or(path);
        write_atomic(&path, b"first\n", 0o600).unwrap();
        assert!(!write_atomic(&path, b"first\n", 0o600).unwrap().changed);
        let result = write_atomic(&path, b"second\n", 0o600).unwrap();
        let backup = result.backup.unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"first\n");
        restore_backup(&path, &backup).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first\n");
        fs::remove_dir_all(directory).unwrap();
    }
}
