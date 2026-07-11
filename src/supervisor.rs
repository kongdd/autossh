use std::{
    fs,
    io::BufRead,
    io::BufReader,
    path::{Path, PathBuf},
    process::Child,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::config::{Config, ConnectionConfig};
use crate::logger::Logger;
use crate::ssh;

const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(2);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Runs all enabled connections until `stop` is set. Configuration changes
/// restart all workers with the latest valid definitions.
pub fn run(config_path: PathBuf, stop: Arc<AtomicBool>) -> Result<()> {
    let mut config = Config::load(&config_path)?;
    let mut logger = Logger::new(&config_path, &config.log)?;
    logger.info(format!(
        "loaded {} connection(s) from {}",
        config.connections.len(),
        config_path.display()
    ));
    let mut supervisor = Supervisor::start(config, logger.clone())?;
    let mut snapshot = config_snapshot(&config_path);
    let mut last_reload_error = None;

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(CONFIG_POLL_INTERVAL);
        let current_snapshot = config_snapshot(&config_path);
        if current_snapshot != snapshot {
            snapshot = current_snapshot;
            match Config::load(&config_path) {
                Ok(new_config) => {
                    logger.info("configuration changed; restarting connection workers");
                    supervisor.stop_and_join();
                    config = new_config;
                    logger = Logger::new(&config_path, &config.log)?;
                    supervisor = Supervisor::start(config.clone(), logger.clone())?;
                    last_reload_error = None;
                }
                Err(error) => {
                    let message = error.to_string();
                    if last_reload_error.as_deref() != Some(message.as_str()) {
                        logger.error(format!(
                            "configuration reload rejected; keeping current connections: {message}"
                        ));
                        last_reload_error = Some(message);
                    }
                }
            }
        }
    }
    logger.info("shutdown requested; stopping connection workers");
    supervisor.stop_and_join();
    Ok(())
}

pub(crate) fn config_snapshot(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

struct Supervisor {
    stop: Arc<AtomicBool>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Supervisor {
    fn start(config: Config, logger: Logger) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();
        for connection in config
            .connections
            .into_iter()
            .filter(|connection| connection.enabled)
        {
            let name = connection.name.clone();
            let worker_stop = Arc::clone(&stop);
            let worker_logger = logger.clone();
            let worker = thread::Builder::new()
                .name(format!("connection-{name}"))
                .spawn(move || supervise_connection(connection, worker_stop, worker_logger))
                .with_context(|| format!("cannot start worker for connection {name}"))?;
            workers.push(worker);
        }
        Ok(Self { stop, workers })
    }

    fn stop_and_join(self) {
        self.stop.store(true, Ordering::SeqCst);
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

fn supervise_connection(connection: ConnectionConfig, stop: Arc<AtomicBool>, logger: Logger) {
    let mut delay = connection.retry.initial_seconds;
    logger.info(format!("{}: supervisor started", connection.name));
    while !stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let mut shutdown = false;
        match ssh::spawn(&connection) {
            Ok(mut child) => {
                logger.info(format!(
                    "{}: ssh process started (pid {})",
                    connection.name,
                    child.id()
                ));
                let stderr_reader =
                    capture_stderr(&mut child, connection.name.clone(), logger.clone());
                match wait_child(&mut child, &stop) {
                    Ok(Some(status)) => {
                        logger.warn(format!("{}: ssh exited with {status}", connection.name))
                    }
                    Ok(None) => {
                        terminate_child(&mut child, &connection.name, &logger);
                        shutdown = true;
                    }
                    Err(error) => {
                        logger.error(format!(
                            "{}: cannot wait for ssh: {error:#}",
                            connection.name
                        ));
                        terminate_child(&mut child, &connection.name, &logger);
                    }
                }
                if let Some(reader) = stderr_reader {
                    let _ = reader.join();
                }
            }
            Err(error) => logger.error(format!("{}: cannot start ssh: {error:#}", connection.name)),
        }
        if shutdown {
            break;
        }

        if started.elapsed() >= Duration::from_secs(connection.retry.stable_seconds) {
            delay = connection.retry.initial_seconds;
            logger.info(format!(
                "{}: stable connection; retry delay reset",
                connection.name
            ));
        }
        logger.info(format!("{}: reconnecting in {delay}s", connection.name));
        if sleep_until_stopped(Duration::from_secs(delay), &stop) {
            break;
        }
        delay = delay
            .saturating_mul(2)
            .min(connection.retry.maximum_seconds);
    }
    logger.info(format!("{}: supervisor stopped", connection.name));
}

fn wait_child(child: &mut Child, stop: &AtomicBool) -> Result<Option<std::process::ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait().context("cannot poll ssh process")? {
            return Ok(Some(status));
        }
        if stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        thread::sleep(CHILD_POLL_INTERVAL);
    }
}

fn capture_stderr(
    child: &mut Child,
    name: String,
    logger: Logger,
) -> Option<thread::JoinHandle<()>> {
    let stderr = child.stderr.take()?;
    thread::Builder::new()
        .name(format!("ssh-stderr-{name}"))
        .spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => logger.warn(format!("{name}: ssh: {line}")),
                    Err(error) => {
                        logger.warn(format!("{name}: cannot read ssh stderr: {error}"));
                        break;
                    }
                }
            }
        })
        .ok()
}

fn terminate_child(child: &mut Child, name: &str, logger: &Logger) {
    if let Err(error) = child.kill() {
        logger.warn(format!("{name}: cannot terminate ssh process: {error}"));
    }
    let _ = child.wait();
}

fn sleep_until_stopped(delay: Duration, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        thread::sleep((deadline - Instant::now()).min(CHILD_POLL_INTERVAL));
    }
    false
}
