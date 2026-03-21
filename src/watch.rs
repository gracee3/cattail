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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct WatchRoot {
    path: PathBuf,
    recursive: bool,
}

#[derive(Clone)]
struct WatchSpec {
    kind: SpecKind,
    root: WatchRoot,
}

#[derive(Clone)]
enum SpecKind {
    Literal(PathBuf),
    Glob(Pattern),
}

impl WatchSpec {
    fn matches(&self, path: &Path, cwd: &Path) -> bool {
        match &self.kind {
            SpecKind::Literal(expected) => {
                normalized_path_key(path, cwd) == normalized_path_key(expected, cwd)
            }
            SpecKind::Glob(pattern) => path_candidates(path, cwd)
                .into_iter()
                .any(|candidate| pattern.matches_path(&candidate)),
        }
    }
}

struct WorkerEntry {
    notify: Arc<Notify>,
    handle: tokio::task::JoinHandle<Result<()>>,
}

struct WatchPlan {
    specs: Vec<WatchSpec>,
    roots: Vec<WatchRoot>,
}

struct Coordinator {
    config: Config,
    cwd: PathBuf,
    tx: mpsc::Sender<OutputLine>,
    handle: Handle,
    specs: Vec<WatchSpec>,
    labels: LabelRegistry,
    workers: HashMap<PathBuf, WorkerEntry>,
    watched_roots: HashSet<WatchRoot>,
    pending_paths: HashMap<PathBuf, PathBuf>,
    pending_wakes: HashSet<PathBuf>,
}

pub fn start(config: Config, tx: mpsc::Sender<OutputLine>) -> Result<WatchRuntime> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let plan = build_watch_plan(&config.inputs, &cwd)?;
    let startup_files =
        resolve_inputs(&config.inputs).context("failed to resolve startup files")?;
    let (stop_tx, stop_rx) = mpsc::channel();
    let handle = Handle::current();

    let join = thread::spawn(move || {
        run_coordinator(config, cwd, startup_files, plan, tx, handle, stop_rx)
    });
    Ok(WatchRuntime { stop_tx, join })
}

fn run_coordinator(
    config: Config,
    cwd: PathBuf,
    startup_files: Vec<PathBuf>,
    plan: WatchPlan,
    tx: mpsc::Sender<OutputLine>,
    handle: Handle,
    stop_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let (event_tx, event_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(event_tx)?;
    let mut coordinator = Coordinator {
        labels: LabelRegistry::with_paths(&startup_files, config.prefix),
        config,
        cwd,
        tx,
        handle,
        specs: plan.specs,
        workers: HashMap::new(),
        watched_roots: HashSet::new(),
        pending_paths: HashMap::new(),
        pending_wakes: HashSet::new(),
    };

    coordinator.watch_roots(&mut watcher, &plan.roots)?;
    coordinator.attach_startup_files(startup_files);

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        match event_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(event)) => {
                coordinator.ingest_event(event);
                coordinator.drain_event_queue(&event_rx);
                coordinator.flush_pending()?;
            }
            Ok(Err(err)) => eprintln!("cattail: watch error: {err}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                coordinator.flush_pending()?;
                coordinator.refresh_from_scan()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    coordinator.shutdown();
    drop(watcher);
    Ok(())
}

impl Coordinator {
    fn watch_roots(&mut self, watcher: &mut impl Watcher, roots: &[WatchRoot]) -> Result<()> {
        for root in roots {
            let root = normalize_watch_root(root.clone(), &self.cwd);
            if self.watched_roots.insert(root.clone()) {
                let mode = if root.recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                watcher.watch(&root.path, mode)?;
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

    fn ingest_event(&mut self, event: notify::Event) {
        for path in event.paths {
            let key = normalized_path_key(&path, &self.cwd);
            self.pending_paths.entry(key).or_insert(path);
        }
    }

    fn drain_event_queue(&mut self, event_rx: &mpsc::Receiver<notify::Result<notify::Event>>) {
        while let Ok(next) = event_rx.try_recv() {
            match next {
                Ok(event) => self.ingest_event(event),
                Err(err) => eprintln!("cattail: watch error: {err}"),
            }
        }
    }

    fn flush_pending(&mut self) -> Result<()> {
        if self.pending_paths.is_empty() && self.pending_wakes.is_empty() {
            return Ok(());
        }

        let pending_paths = std::mem::take(&mut self.pending_paths);
        for (key, path) in pending_paths {
            if self.workers.contains_key(&key) {
                self.pending_wakes.insert(key);
                continue;
            }

            if self.specs.iter().any(|spec| spec.matches(&path, &self.cwd)) {
                let _ = self.attach(path, self.start_mode_for_dynamic());
            }
        }

        let pending_wakes = std::mem::take(&mut self.pending_wakes);
        for key in pending_wakes {
            if let Some(worker) = self.workers.get(&key) {
                worker.notify.notify_one();
            }
        }

        Ok(())
    }

    fn refresh_from_scan(&mut self) -> Result<()> {
        let resolved = resolve_inputs(&self.config.inputs)?;
        for path in resolved {
            let key = normalized_path_key(&path, &self.cwd);
            if self.workers.contains_key(&key) {
                continue;
            }
            let _ = self.attach(path, self.start_mode_for_dynamic());
        }
        Ok(())
    }

    fn attach(&mut self, path: PathBuf, start_mode: StartMode) -> Result<()> {
        let key = normalized_path_key(&path, &self.cwd);
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

fn build_watch_plan(inputs: &[String], cwd: &Path) -> Result<WatchPlan> {
    let mut specs = Vec::with_capacity(inputs.len());
    let mut roots = Vec::new();

    for input in inputs {
        let spec = build_spec(input, cwd)?;
        insert_watch_root(&mut roots, spec.root.clone());
        specs.push(spec);
    }

    Ok(WatchPlan { specs, roots })
}

fn build_spec(input: &str, cwd: &Path) -> Result<WatchSpec> {
    if has_glob_magic(input) {
        Ok(WatchSpec {
            kind: SpecKind::Glob(
                Pattern::new(input).with_context(|| format!("invalid glob pattern: {input}"))?,
            ),
            root: watch_root_for_glob(input, cwd),
        })
    } else {
        Ok(WatchSpec {
            kind: SpecKind::Literal(PathBuf::from(input)),
            root: watch_root_for_literal(input, cwd),
        })
    }
}

fn insert_watch_root(roots: &mut Vec<WatchRoot>, candidate: WatchRoot) {
    if roots.iter().any(|root| root_covers(root, &candidate)) {
        return;
    }

    roots.retain(|root| !root_covers(&candidate, root));
    if let Some(existing) = roots
        .iter_mut()
        .find(|root| same_watch_root(root, &candidate))
    {
        existing.recursive |= candidate.recursive;
        return;
    }

    roots.push(candidate);
}

fn same_watch_root(a: &WatchRoot, b: &WatchRoot) -> bool {
    a.path == b.path
}

fn root_covers(base: &WatchRoot, other: &WatchRoot) -> bool {
    if base.path == other.path {
        return base.recursive || !other.recursive;
    }

    if !base.recursive {
        return false;
    }

    other.path.strip_prefix(&base.path).is_ok()
}

fn watch_root_for_literal(input: &str, cwd: &Path) -> WatchRoot {
    let path = Path::new(input);
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    normalize_watch_root(
        WatchRoot {
            path: root,
            recursive: false,
        },
        cwd,
    )
}

fn watch_root_for_glob(input: &str, cwd: &Path) -> WatchRoot {
    let path = Path::new(input);
    let mut root = PathBuf::new();
    let mut recursive = false;
    let components: Vec<_> = path.components().collect();

    for (idx, component) in components.iter().enumerate() {
        let piece = component.as_os_str().to_string_lossy();
        if has_glob_magic(&piece) {
            recursive = idx + 1 < components.len() || piece.contains("**");
            break;
        }
        root.push(component.as_os_str());
    }

    if root.as_os_str().is_empty() {
        root = PathBuf::from(".");
    }

    normalize_watch_root(
        WatchRoot {
            path: root,
            recursive,
        },
        cwd,
    )
}

fn normalize_watch_root(root: WatchRoot, cwd: &Path) -> WatchRoot {
    let recursive = root.recursive;
    let path = if root.path.is_absolute() {
        root.path
    } else {
        cwd.join(root.path)
    };

    let path = if path.exists() {
        std::fs::canonicalize(&path).unwrap_or(path)
    } else {
        path
    };

    WatchRoot { path, recursive }
}

fn has_glob_magic(input: &str) -> bool {
    input.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

fn path_candidates(path: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    candidates.push(path.to_path_buf());

    if path.is_absolute() {
        if let Ok(relative) = path.strip_prefix(cwd) {
            candidates.push(relative.to_path_buf());
        }
    } else {
        candidates.push(cwd.join(path));
    }

    candidates.sort();
    candidates.dedup();
    candidates
}

fn normalized_path_key(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    if absolute.exists() {
        std::fs::canonicalize(&absolute).unwrap_or(absolute)
    } else {
        absolute
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::thread;
    use tempfile::tempdir;
    use tokio::sync::mpsc as tokio_mpsc;
    use tokio::time::Duration;

    #[test]
    fn parses_glob_and_literal_specs() {
        let cwd = PathBuf::from("/tmp/cattail-test");
        let plan =
            build_watch_plan(&["logs/*.log".to_string(), "app.log".to_string()], &cwd).unwrap();
        assert_eq!(plan.specs.len(), 2);
        assert!(matches!(plan.specs[0].kind, SpecKind::Glob(_)));
        assert!(matches!(plan.specs[1].kind, SpecKind::Literal(_)));
    }

    #[test]
    fn root_planner_uses_narrow_roots() {
        let dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir(&logs).unwrap();

        let plan = build_watch_plan(
            &[
                format!("{}/{}.log", logs.display(), "*"),
                format!("{}/**/*.txt", logs.display()),
            ],
            dir.path(),
        )
        .unwrap();

        assert_eq!(plan.roots.len(), 1);
        assert_eq!(plan.roots[0].path, fs::canonicalize(&logs).unwrap());
        assert!(plan.roots[0].recursive);
    }

    #[test]
    fn glob_overlap_matches_existing_only_once() {
        let cwd = PathBuf::from("/tmp/cattail-test");
        let a = WatchSpec {
            kind: SpecKind::Glob(Pattern::new("logs/*.log").unwrap()),
            root: WatchRoot {
                path: cwd.join("logs"),
                recursive: false,
            },
        };
        let b = WatchSpec {
            kind: SpecKind::Glob(Pattern::new("logs/or*.log").unwrap()),
            root: WatchRoot {
                path: cwd.join("logs"),
                recursive: false,
            },
        };
        let path = cwd.join("logs/orcas.log");
        assert!(a.matches(&path, &cwd));
        assert!(b.matches(&path, &cwd));
        let mut registry = HashSet::new();
        let key = normalized_path_key(&path, &cwd);
        assert!(registry.insert(key.clone()));
        assert!(!registry.insert(key));
    }

    #[test]
    fn recursive_glob_plans_recursive_watch() {
        let cwd = PathBuf::from("/tmp/cattail-test");
        let spec = build_watch_plan(&["logs/**/*.log".to_string()], &cwd)
            .unwrap()
            .specs
            .remove(0);
        assert!(spec.root.recursive);
        assert_eq!(spec.root.path, cwd.join("logs"));
    }

    #[test]
    fn watch_root_for_literal_uses_parent_directory() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("app.log");
        fs::write(&file, "x\n").unwrap();
        let spec = build_watch_plan(&[file.display().to_string()], dir.path())
            .unwrap()
            .specs
            .remove(0);
        assert_eq!(spec.root.path, dir.path());
        assert!(!spec.root.recursive);
    }

    #[test]
    fn path_key_canonicalizes_existing_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.log");
        fs::write(&path, "x\n").unwrap();
        let key = normalized_path_key(&path, dir.path());
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

    #[tokio::test]
    async fn startup_and_following_stay_single_stream() {
        let dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let file = logs.join("app.log");
        fs::write(&file, "boot\n").unwrap();

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

        let backlog = wait_for_line(&mut rx).await;
        assert_eq!(backlog.line, "boot");

        let mut file_handle = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(file_handle, "live-1").unwrap();
        writeln!(file_handle, "live-2").unwrap();

        let first = wait_for_line(&mut rx).await;
        let second = wait_for_line(&mut rx).await;
        assert_eq!(first.line, "live-1");
        assert_eq!(second.line, "live-2");
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
