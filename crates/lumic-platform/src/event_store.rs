use lumic_core::{LumicError, Result, events::Event};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

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
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&self.path).map_err(io_error)?;
        serde_json::to_writer(&mut file, event).map_err(json_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_data().map_err(io_error)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(io_error)?;
        let mut events = BufReader::new(file)
            .lines()
            .map(|line| {
                let line = line.map_err(io_error)?;
                serde_json::from_str(&line).map_err(json_error)
            })
            .collect::<Result<Vec<_>>>()?;
        if events.len() > limit {
            events.drain(..events.len() - limit);
        }
        events.reverse();
        Ok(events)
    }
}

fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("event store I/O failed: {error}"),
    }
}

fn json_error(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("event store data is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::OperationInterface;

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
