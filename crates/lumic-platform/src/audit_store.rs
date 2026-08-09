use crate::jsonl_store;
use lumic_core::{Result, events::AuditRecord};
use std::path::{Path, PathBuf};

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
        jsonl_store::append(&self.path, record, "audit")
    }

    pub fn list(&self, limit: usize) -> Result<Vec<AuditRecord>> {
        jsonl_store::latest(&self.path, limit, "audit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{OperationContext, OperationInterface};
    use serde_json::json;
    use std::fs;

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
