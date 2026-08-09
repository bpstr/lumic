use lumic_core::{LumicError, Result};
use serde::{Serialize, de::DeserializeOwned};
#[cfg(unix)]
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
};

pub(crate) fn append<T: Serialize>(path: &Path, value: &T, name: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(name, error))?;
    }
    let mut line = serde_json::to_vec(value).map_err(|error| json_error(name, error))?;
    line.push(b'\n');

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|error| io_error(name, error))?;
    ensure_regular_file(&file, name)?;
    lock(&file, LockMode::Exclusive, name)?;
    file.write_all(&line)
        .and_then(|_| file.sync_data())
        .map_err(|error| io_error(name, error))
}

pub(crate) fn latest<T: DeserializeOwned>(path: &Path, limit: usize, name: &str) -> Result<Vec<T>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(name, error)),
    };
    ensure_regular_file(&file, name)?;
    lock(&file, LockMode::Shared, name)?;

    let mut values = VecDeque::with_capacity(limit.min(1024));
    for line in BufReader::new(file).lines() {
        let value = serde_json::from_str(&line.map_err(|error| io_error(name, error))?)
            .map_err(|error| json_error(name, error))?;
        if values.len() == limit {
            values.pop_front();
        }
        values.push_back(value);
    }
    Ok(values.into_iter().rev().collect())
}

fn ensure_regular_file(file: &File, name: &str) -> Result<()> {
    if !file
        .metadata()
        .map_err(|error| io_error(name, error))?
        .is_file()
    {
        return Err(LumicError::Internal {
            message: format!("{name} store path is not a regular file"),
        });
    }
    Ok(())
}

#[cfg(unix)]
fn lock(file: &File, mode: LockMode, name: &str) -> Result<()> {
    let operation = match mode {
        LockMode::Shared => libc::LOCK_SH,
        LockMode::Exclusive => libc::LOCK_EX,
    };
    // SAFETY: `file` owns a valid descriptor for the duration of this call.
    if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
        Ok(())
    } else {
        Err(io_error(name, std::io::Error::last_os_error()))
    }
}

#[cfg(not(unix))]
fn lock(_file: &File, _mode: LockMode, _name: &str) -> Result<()> {
    Ok(())
}

enum LockMode {
    Shared,
    Exclusive,
}

fn io_error(name: &str, error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("{name} store I/O failed: {error}"),
    }
}

fn json_error(name: &str, error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("{name} store data is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "lumic-jsonl-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn concurrent_appends_remain_complete_json_lines_and_tail_is_bounded() {
        let directory = temp_path("concurrent");
        let path = Arc::new(directory.join("records.jsonl"));
        let threads = (0..8)
            .map(|worker| {
                let path = Arc::clone(&path);
                thread::spawn(move || {
                    for value in 0..50 {
                        append(&path, &(worker, value), "test").unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for handle in threads {
            handle.join().unwrap();
        }

        let all: Vec<(usize, usize)> = latest(&path, 400, "test").unwrap();
        let tail: Vec<(usize, usize)> = latest(&path, 17, "test").unwrap();
        assert_eq!(all.len(), 400);
        assert_eq!(tail.len(), 17);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_store_paths() {
        use std::os::unix::fs::symlink;

        let directory = temp_path("symlink");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("target.jsonl");
        fs::write(&target, b"1\n").unwrap();
        let path = directory.join("records.jsonl");
        symlink(&target, &path).unwrap();

        assert!(latest::<usize>(&path, 1, "test").is_err());
        assert!(append(&path, &2, "test").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
