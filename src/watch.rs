use crate::cli::Config;
use crate::follow::{self, StartMode, WorkerConfig};
use crate::output::{LabelRegistry, OutputLine};
use crate::resolve::resolve_inputs;
use anyhow::{Context, Result};
use glob::Pattern;
use notify::{RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::Notify;

pub struct WatchRuntime {
    stop_tx: mpsc::Sender<()>,
    join: thread::JoinHandle<Result<()>>,
}

impl WatchRuntime {
    pub fn shutdown(self) -> Result<()> {
        let _ = self.stop_tx.send(());
        match self.join.join() {
            Ok(result) => result,
            Err(_) => anyhow::bail!("watch coordinator thread panicked"),
        }
    }
}

#[derive(Clone)]
struct WatchSpec {
    kind: SpecKind,
    root: PathBuf,
}

#[derive(Clone)]
enum SpecKind {
    Literal(PathBuf),
    Glob(Pattern),
}

impl WatchSpec {
    fn matches(&self, path: &Path) -> bool {
        match &self.kind {
            SpecKind::Literal(expected) => path_key(path) == path_key(expected),
            SpecKind::Glob(pattern) => pattern.matches_path(path),
        }
    }
}

struct WorkerEntry {
    notify: Arc<Notify>,
    handle: tokio::task::JoinHandle<Result<()>>,
}

struct Coordinator {
    config: Config,
    tx: mpsc::Sender<OutputLine>,
    handle: Handle,
    specs: Vec<WatchSpec>,
    labels: LabelRegistry,
    workers: HashMap<PathBuf, WorkerEntry>,
    watched_roots: HashSet<PathBuf>,
}

pub fn start(config: Config, tx: mpsc::Sender<OutputLine>) -> Result<WatchRuntime> {
    let startup_files =
        resolve_inputs(&config.inputs).context("failed to resolve startup files")?;
    let specs = parse_specs(&config.inputs)?;
    let (stop_tx, stop_rx) = mpsc::channel();
    let handle = Handle::current();

    let join =
        thread::spawn(move || run_coordinator(config, startup_files, specs, tx, handle, stop_rx));
    Ok(WatchRuntime { stop_tx, join })
}

fn run_coordinator(
    config: Config,
    startup_files: Vec<PathBuf>,
    specs: Vec<WatchSpec>,
    tx: mpsc::Sender<OutputLine>,
    handle: Handle,
    stop_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let (event_tx, event_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(event_tx)?;
    let mut coordinator = Coordinator {
        labels: LabelRegistry::with_paths(&startup_files, config.prefix),
        config,
        tx,
        handle,
        specs,
        workers: HashMap::new(),
        watched_roots: HashSet::new(),
    };

    coordinator.watch_roots(&mut watcher)?;
    coordinator.attach_startup_files(startup_files);

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        match event_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(event)) => coordinator.process_event(event),
            Ok(Err(err)) => eprintln!("cattail: watch error: {err}"),
            Err(mpsc::RecvTimeoutError::Timeout) => coordinator.refresh_from_scan()?,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    coordinator.shutdown();
    drop(watcher);
    Ok(())
}

impl Coordinator {
    fn watch_roots(&mut self, watcher: &mut impl Watcher) -> Result<()> {
        for spec in &self.specs {
            let root = spec.root.clone();
            if self.watched_roots.insert(root.clone()) {
                watcher.watch(&root, RecursiveMode::Recursive)?;
            }
        }
        Ok(())
    }

    fn attach_startup_files(&mut self, startup_files: Vec<PathBuf>) {
        for path in startup_files {
            let _ = self.attach(path, self.start_mode_for_startup());
        }
    }

    fn start_mode_for_startup(&self) -> StartMode {
        if self.config.since_now {
            StartMode::FromCurrentEnd
        } else {
            StartMode::StartupBacklog(self.config.lines)
        }
    }

    fn start_mode_for_dynamic(&self) -> StartMode {
        StartMode::FromBeginning
    }

    fn process_event(&mut self, event: notify::Event) {
        for path in event.paths {
            let key = path_key(&path);
            if let Some(worker) = self.workers.get(&key) {
                worker.notify.notify_one();
                continue;
            }

            if self.specs.iter().any(|spec| spec.matches(&path)) {
                let _ = self.attach(path, self.start_mode_for_dynamic());
            }
        }
    }

    fn refresh_from_scan(&mut self) -> Result<()> {
        let resolved = resolve_inputs(&self.config.inputs)?;
        for path in resolved {
            let key = path_key(&path);
            if self.workers.contains_key(&key) {
                continue;
            }
            let _ = self.attach(path, self.start_mode_for_dynamic());
        }
        Ok(())
    }

    fn attach(&mut self, path: PathBuf, start_mode: StartMode) -> Result<()> {
        let key = path_key(&path);
        if self.workers.contains_key(&key) {
            return Ok(());
        }

        let label = self.labels.allocate(&path);
        let notify = Arc::new(Notify::new());
        let worker_notify = notify.clone();
        let tx = self.tx.clone();
        let interval = self.config.interval();
        let join = self.handle.spawn(async move {
            follow::run_worker(WorkerConfig {
                path,
                label,
                start_mode,
                interval,
                wake: worker_notify,
                tx,
            })
            .await
        });

        self.workers.insert(
            key,
            WorkerEntry {
                notify,
                handle: join,
            },
        );
        Ok(())
    }

    fn shutdown(&mut self) {
        for worker in self.workers.values() {
            worker.handle.abort();
        }
    }
}

fn parse_specs(inputs: &[String]) -> Result<Vec<WatchSpec>> {
    let mut specs = Vec::with_capacity(inputs.len());
    for input in inputs {
        let kind = if has_glob_magic(input) {
            SpecKind::Glob(
                Pattern::new(input).with_context(|| format!("invalid glob pattern: {input}"))?,
            )
        } else {
            SpecKind::Literal(PathBuf::from(input))
        };
        let root = watch_root_for_input(input);
        specs.push(WatchSpec { kind, root });
    }
    Ok(specs)
}

fn watch_root_for_input(input: &str) -> PathBuf {
    let path = Path::new(input);
    if has_glob_magic(input) {
        let anchor = glob_anchor(path);
        if anchor.is_dir() {
            normalize_root(anchor)
        } else {
            normalize_root(
                anchor
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| anchor.to_path_buf()),
            )
        }
    } else {
        normalize_root(
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        )
    }
}

fn normalize_root(root: PathBuf) -> PathBuf {
    if root.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root
    }
}

fn glob_anchor(path: &Path) -> PathBuf {
    let mut anchor = PathBuf::new();
    for component in path.components() {
        let piece = component.as_os_str().to_string_lossy();
        if piece.chars().any(|c| matches!(c, '*' | '?' | '[' | ']')) {
            break;
        }
        anchor.push(component.as_os_str());
    }
    if anchor.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        anchor
    }
}

fn has_glob_magic(input: &str) -> bool {
    input.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

fn path_key(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use tempfile::tempdir;
    use tokio::sync::mpsc as tokio_mpsc;
    use tokio::time::Duration;

    #[test]
    fn parses_glob_and_literal_specs() {
        let specs = parse_specs(&["logs/*.log".to_string(), "app.log".to_string()]).unwrap();
        assert_eq!(specs.len(), 2);
        assert!(matches!(specs[0].kind, SpecKind::Glob(_)));
        assert!(matches!(specs[1].kind, SpecKind::Literal(_)));
    }

    #[test]
    fn glob_overlap_matches_existing_only_once() {
        let a = WatchSpec {
            kind: SpecKind::Glob(Pattern::new("logs/*.log").unwrap()),
            root: PathBuf::from("logs"),
        };
        let b = WatchSpec {
            kind: SpecKind::Glob(Pattern::new("logs/or*.log").unwrap()),
            root: PathBuf::from("logs"),
        };
        let path = PathBuf::from("logs/orcas.log");
        assert!(a.matches(&path));
        assert!(b.matches(&path));
        let mut registry = HashSet::new();
        let key = path_key(&path);
        assert!(registry.insert(key.clone()));
        assert!(!registry.insert(key));
    }

    #[test]
    fn watch_root_uses_parent_ancestor() {
        let spec = parse_specs(&["logs/*.log".to_string()]).unwrap().remove(0);
        assert_eq!(spec.root, PathBuf::from("."));
    }

    #[test]
    fn path_key_canonicalizes_existing_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.log");
        fs::write(&path, "x\n").unwrap();
        let key = path_key(&path);
        assert!(key.is_absolute());
    }

    #[tokio::test]
    async fn discovers_new_matching_file_and_labels_it() {
        let dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir(&logs).unwrap();

        let pattern = format!("{}/{}.log", logs.display(), "*");
        let config = Config {
            lines: 50,
            interval_ms: 25,
            prefix: crate::cli::PrefixMode::Basename,
            since_now: false,
            color: crate::cli::ColorMode::Never,
            inputs: vec![pattern],
        };
        let (tx, rx) = mpsc::channel();
        let runtime = start(config, tx).unwrap();
        let mut rx = forward_receiver(rx);

        tokio::time::sleep(Duration::from_millis(100)).await;
        let file = logs.join("worker.log");
        fs::write(&file, "boot\nready\n").unwrap();

        let first = wait_for_line(&mut rx).await;
        let second = wait_for_line(&mut rx).await;
        assert_eq!(first.label, "worker.log");
        assert_eq!(first.line, "boot");
        assert_eq!(second.line, "ready");

        let mut file_handle = fs::OpenOptions::new().append(true).open(&file).unwrap();
        use std::io::Write;
        writeln!(file_handle, "live").unwrap();
        let third = wait_for_line(&mut rx).await;
        assert_eq!(third.line, "live");

        runtime.shutdown().unwrap();
    }

    #[tokio::test]
    async fn overlapping_globs_attach_once() {
        let dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir(&logs).unwrap();

        let pattern_a = format!("{}/{}.log", logs.display(), "*");
        let pattern_b = format!("{}/or*.log", logs.display());
        let config = Config {
            lines: 50,
            interval_ms: 25,
            prefix: crate::cli::PrefixMode::Basename,
            since_now: false,
            color: crate::cli::ColorMode::Never,
            inputs: vec![pattern_a, pattern_b],
        };
        let (tx, rx) = mpsc::channel();
        let runtime = start(config, tx).unwrap();
        let mut rx = forward_receiver(rx);

        tokio::time::sleep(Duration::from_millis(100)).await;
        let file = logs.join("orcas.log");
        fs::write(&file, "one\n").unwrap();

        let line = wait_for_line(&mut rx).await;
        assert_eq!(line.label, "orcas.log");
        assert_eq!(line.line, "one");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(rx.try_recv().is_err());

        runtime.shutdown().unwrap();
    }

    fn forward_receiver(
        rx: mpsc::Receiver<OutputLine>,
    ) -> tokio_mpsc::UnboundedReceiver<OutputLine> {
        let (tx, out_rx) = tokio_mpsc::unbounded_channel();
        thread::spawn(move || {
            while let Ok(item) = rx.recv() {
                let _ = tx.send(item);
            }
        });
        out_rx
    }

    async fn wait_for_line(rx: &mut tokio_mpsc::UnboundedReceiver<OutputLine>) -> OutputLine {
        match rx.recv().await {
            Some(line) => line,
            None => panic!("receiver closed before emitting a line"),
        }
    }
}
