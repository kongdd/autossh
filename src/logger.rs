use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

use crate::config::LogConfig;

#[derive(Clone)]
pub struct Logger(Arc<Mutex<LogSink>>);

struct LogSink {
    path: Option<PathBuf>,
    max_bytes: u64,
    file: Option<File>,
}

impl Logger {
    pub fn new(config_path: &Path, log: &LogConfig) -> Result<Self> {
        let path = log.file.as_ref().map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                config_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(path)
            }
        });
        let file = match &path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                Some(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .with_context(|| format!("cannot open log file {}", path.display()))?,
                )
            }
            None => None,
        };
        Ok(Self(Arc::new(Mutex::new(LogSink {
            path,
            max_bytes: log.rotate_mib.saturating_mul(1024 * 1024),
            file,
        }))))
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.write("INFO", message.as_ref());
    }
    pub fn warn(&self, message: impl AsRef<str>) {
        self.write("WARN", message.as_ref());
    }
    pub fn error(&self, message: impl AsRef<str>) {
        self.write("ERROR", message.as_ref());
    }

    fn write(&self, level: &str, message: &str) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let line = format!("[{seconds}] {level:<5} {message}\n");
        eprint!("{line}");
        let mut sink = self.0.lock().expect("logger mutex poisoned");
        if let Some(path) = sink.path.clone()
            && sink.max_bytes > 0
            && fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= sink.max_bytes
        {
            // Windows cannot rename file while current process holds it open.
            sink.file = None;
            let rotated = path.with_extension("log.1");
            let _ = fs::remove_file(&rotated);
            if let Err(error) = fs::rename(&path, &rotated) {
                eprintln!("log rotation failed: {error}");
            }
            sink.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok();
        }
        if let Some(file) = &mut sink.file {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}
