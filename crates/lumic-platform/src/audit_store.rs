use lumic_core::{LumicError, Result, events::AuditRecord};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct AuditStore {
    path: PathBuf,
}

impl AuditStore {
    pub fn at_state_dir(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("audit.jsonl"),
        }
    }

    pub fn append(&self, record: &AuditRecord) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&self.path).map_err(io_error)?;
        serde_json::to_writer(&mut file, record).map_err(json_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_data().map_err(io_error)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<AuditRecord>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(io_error)?;
        let mut records = BufReader::new(file)
            .lines()
            .map(|line| serde_json::from_str(&line.map_err(io_error)?).map_err(json_error))
            .collect::<Result<Vec<_>>>()?;
        if records.len() > limit {
            records.drain(..records.len() - limit);
        }
        records.reverse();
        Ok(records)
    }
}

fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("audit store I/O failed: {error}"),
    }
}

fn json_error(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("audit store data is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{OperationContext, OperationInterface};
    use serde_json::json;

    #[test]
    fn persists_structured_before_and_after_state() {
        let directory = std::env::temp_dir().join(format!("lumic-audit-{}", std::process::id()));
        let store = AuditStore::at_state_dir(&directory);
        let context = OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "test-1".into(),
            dry_run: false,
            approved: true,
        };
        store
            .append(&AuditRecord::now(
                &context,
                "service.restart",
                "restart",
                "service",
                "nginx",
                json!({}),
                Some(json!({"active": false})),
                Some(json!({"active": true})),
                true,
                "restarted",
            ))
            .unwrap();
        let records = store.list(1).unwrap();
        assert_eq!(records[0].capability, "service.restart");
        assert!(records[0].before.is_some());
        fs::remove_dir_all(directory).unwrap();
    }
}
