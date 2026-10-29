//! Bounded local JSONL diagnostics. Never record book text, titles or paths.
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: u64 = 2 * 1024 * 1024;
const ARCHIVES: usize = 3;
static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

struct Log {
    directory: PathBuf,
    limit: u64,
}
impl Log {
    fn path(&self, index: usize) -> PathBuf {
        self.directory.join(if index == 0 {
            "reader.jsonl".into()
        } else {
            format!("reader.{index}.jsonl")
        })
    }
    fn write(&self, value: &Value) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        if fs::metadata(self.path(0)).is_ok_and(|m| m.len() + bytes.len() as u64 > self.limit) {
            for index in (1..=ARCHIVES).rev() {
                let destination = self.path(index);
                if destination.exists() {
                    fs::remove_file(&destination)?;
                }
                let source = self.path(index - 1);
                if source.exists() {
                    fs::rename(source, destination)?;
                }
            }
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(0))?
            .write_all(&bytes)
    }
}

pub fn init(directory: PathBuf) {
    let _ = LOG.set(Mutex::new(Log {
        directory,
        limit: LIMIT,
    }));
    record(
        "process-start",
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
        }),
    );
}

pub fn record(event: &str, details: Value) {
    if let Some(log) = LOG.get() {
        if let Ok(log) = log.lock() {
            let _ = log.write(&json!({
                "timeMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
                "pid": std::process::id(),
                "event": event,
                "details": details,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_rotate_and_preserve_complete_json_records() {
        let directory =
            std::env::temp_dir().join(format!("fishbaby-log-test-{}", std::process::id()));
        let log = Log {
            directory,
            limit: 80,
        };
        for index in 0..30 {
            log.write(&json!({ "index": index, "event": "snapshot" }))
                .unwrap();
        }
        let files = fs::read_dir(&log.directory).unwrap().count();
        assert_eq!(files, ARCHIVES + 1);
        for index in 0..=ARCHIVES {
            let path = log.path(index);
            assert!(fs::metadata(&path).unwrap().len() <= log.limit);
            for line in fs::read_to_string(path).unwrap().lines() {
                serde_json::from_str::<Value>(line).unwrap();
            }
        }
        let latest = fs::read_to_string(log.path(0)).unwrap();
        assert!(latest.contains("29"));
        fs::remove_dir_all(log.directory).unwrap();
    }
}
