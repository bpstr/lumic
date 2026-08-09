use crate::jsonl_store;
use lumic_core::{Result, events::Event};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EventStore {
    path: PathBuf,
}

impl EventStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn at_state_dir(directory: impl AsRef<Path>) -> Self {
        Self::new(directory.as_ref().join("events.jsonl"))
    }

    pub fn state_dir(&self) -> &Path {
        self.path
            .parent()
            .expect("event store path always has a parent directory")
    }

    pub fn append(&self, event: &Event) -> Result<()> {
        jsonl_store::append(&self.path, event, "event")
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Event>> {
        jsonl_store::latest(&self.path, limit, "event")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::OperationInterface;
    use std::fs;

    #[test]
    fn persists_and_reads_newest_events_first() {
        let directory = std::env::temp_dir().join(format!(
            "lumic-events-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = EventStore::at_state_dir(&directory);
        for event_type in ["first", "second"] {
            store
                .append(&Event::now(
                    event_type,
                    "test",
                    OperationInterface::Internal,
                    "test",
                    "1",
                    "test-1",
                    serde_json::json!({}),
                ))
                .unwrap();
        }
        let events = store.list(1).unwrap();
        assert_eq!(events[0].event_type, "second");
        fs::remove_dir_all(directory).unwrap();
    }
}
