use crate::output::OutputLine;
use crate::tail;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::Notify;
use tokio::time::{self, Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartMode {
    StartupBacklog(usize),
    FromBeginning,
    FromCurrentEnd,
}

pub struct FollowState {
    path: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    unavailable: bool,
}

impl FollowState {
    pub fn new(path: PathBuf, offset: u64) -> Self {
        Self {
            path,
            offset,
            pending: Vec::new(),
            unavailable: false,
        }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn truncation_detected(&self, len: u64) -> bool {
        len < self.offset
    }

    pub fn reset_for_truncation(&mut self) {
        self.offset = 0;
        self.pending.clear();
    }

    pub fn reset_for_reopen(&mut self) {
        // Treat a delete/recreate event as a fresh file and read from the
        // beginning as soon as it reappears.
        self.offset = 0;
        self.pending.clear();
    }

    fn ingest(&mut self, bytes: &[u8], flush_partial: bool) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();

        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let mut line = self.pending.drain(..=pos).collect::<Vec<u8>>();
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            out.push(String::from_utf8_lossy(&line).to_string());
        }

        if flush_partial && !self.pending.is_empty() {
            let mut line = std::mem::take(&mut self.pending);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            out.push(String::from_utf8_lossy(&line).to_string());
        }

        out
    }
}

pub async fn initial_backlog(path: &Path, lines: usize) -> Result<(Vec<String>, u64)> {
    let backlog = tail::read_last_lines(path, lines)?;
    let len = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("reading metadata for {path:?}"))?
        .len();
    Ok((backlog, len))
}

pub struct WorkerConfig {
    pub path: PathBuf,
    pub label: String,
    pub start_mode: StartMode,
    pub interval: Duration,
    pub wake: Arc<Notify>,
    pub tx: mpsc::Sender<OutputLine>,
}

pub async fn run_worker(config: WorkerConfig) -> Result<()> {
    let (mut state, backlog) = initialize_state(&config.path, config.start_mode).await?;
    for line in backlog {
        let _ = config.tx.send(OutputLine {
            label: config.label.clone(),
            line,
        });
    }

    let _ = poll_once(&mut state, &config.label, &config.tx).await;

    let mut tick = time::interval(config.interval);
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let _ = poll_once(&mut state, &config.label, &config.tx).await;
            }
            _ = config.wake.notified() => {
                let _ = poll_once(&mut state, &config.label, &config.tx).await;
            }
        }
    }
}

async fn initialize_state(
    path: &Path,
    start_mode: StartMode,
) -> Result<(FollowState, Vec<String>)> {
    let mut state = FollowState::new(path.to_path_buf(), 0);
    let mut backlog = Vec::new();

    match start_mode {
        StartMode::StartupBacklog(lines) => {
            let (lines_out, len, unavailable) = startup_backlog_state(path, lines).await;
            backlog = lines_out;
            state.offset = len;
            state.unavailable = unavailable;
        }
        StartMode::FromBeginning => {}
        StartMode::FromCurrentEnd => match tokio::fs::metadata(path).await {
            Ok(meta) => {
                state.offset = meta.len();
            }
            Err(err) => {
                eprintln!("cattail: {}: {err:#}", path.display());
                state.unavailable = true;
            }
        },
    }

    Ok((state, backlog))
}

async fn startup_backlog_state(path: &Path, lines: usize) -> (Vec<String>, u64, bool) {
    match initial_backlog(path, lines).await {
        Ok((backlog, len)) => (backlog, len, false),
        Err(err) => {
            eprintln!("cattail: {}: {err:#}", path.display());
            (Vec::new(), 0, true)
        }
    }
}

pub async fn watch_file(
    path: PathBuf,
    label: String,
    lines: usize,
    interval: Duration,
    tx: mpsc::Sender<OutputLine>,
) -> Result<()> {
    run_worker(WorkerConfig {
        path,
        label,
        start_mode: StartMode::StartupBacklog(lines),
        interval,
        wake: Arc::new(Notify::new()),
        tx,
    })
    .await
}

pub async fn poll_once(
    state: &mut FollowState,
    label: &str,
    tx: &mpsc::Sender<OutputLine>,
) -> Result<()> {
    let meta = match tokio::fs::metadata(&state.path).await {
        Ok(meta) => meta,
        Err(err) => {
            if !state.unavailable {
                eprintln!("cattail: {}: {err}", state.path.display());
                state.unavailable = true;
            }
            return Ok(());
        }
    };

    let len = meta.len();
    if state.unavailable {
        state.reset_for_reopen();
        state.unavailable = false;
    }

    if state.truncation_detected(len) {
        state.reset_for_truncation();
    }

    if len <= state.offset {
        return Ok(());
    }

    let mut file = match tokio::fs::File::open(&state.path).await {
        Ok(file) => file,
        Err(err) => {
            if !state.unavailable {
                eprintln!("cattail: {}: {err}", state.path.display());
                state.unavailable = true;
            }
            return Ok(());
        }
    };
    if let Err(err) = file.seek(SeekFrom::Start(state.offset)).await {
        if !state.unavailable {
            eprintln!("cattail: {}: {err}", state.path.display());
            state.unavailable = true;
        }
        return Ok(());
    }
    let mut buf = Vec::new();
    if let Err(err) = file.read_to_end(&mut buf).await {
        if !state.unavailable {
            eprintln!("cattail: {}: {err}", state.path.display());
            state.unavailable = true;
        }
        return Ok(());
    }
    state.offset = len;

    for line in state.ingest(&buf, false) {
        let _ = tx.send(OutputLine {
            label: label.to_string(),
            line,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use tempfile::tempdir;
    use tokio::time::Duration;

    #[test]
    fn truncation_resets_state() {
        let mut state = FollowState::new(PathBuf::from("/tmp/x.log"), 128);
        state.pending.extend_from_slice(b"hello");
        assert!(state.truncation_detected(0));
        state.reset_for_truncation();
        assert_eq!(state.offset(), 0);
        assert_eq!(state.pending_len(), 0);
    }

    #[test]
    fn buffers_partial_lines_until_newline() {
        let mut state = FollowState::new(PathBuf::from("/tmp/x.log"), 0);
        let out = state.ingest(b"abc", false);
        assert!(out.is_empty());
        let out = state.ingest(b"def\nxyz\n", false);
        assert_eq!(out, vec!["abcdef".to_string(), "xyz".to_string()]);
    }

    #[tokio::test]
    async fn integration_style_follow_reads_backlog_and_appends() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.log");
        fs::write(&path, "one\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let (backlog, len) = initial_backlog(&path, 50).await.unwrap();
        assert_eq!(backlog, vec!["one".to_string()]);

        let mut state = FollowState::new(path.clone(), len);
        fs::write(&path, "one\ntwo\n").unwrap();
        poll_once(&mut state, "app.log", &tx).await.unwrap();

        let item = rx.try_recv().unwrap();
        assert_eq!(item.label, "app.log");
        assert_eq!(item.line, "two");
    }

    #[tokio::test]
    async fn recreated_file_is_read_from_beginning() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recreate.log");
        fs::write(&path, "old\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut state = FollowState::new(path.clone(), 999);
        state.unavailable = true;

        fs::write(&path, "fresh-a\nfresh-b\n").unwrap();
        poll_once(&mut state, "recreate.log", &tx).await.unwrap();
        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();
        assert_eq!(first.line, "fresh-a");
        assert_eq!(second.line, "fresh-b");
    }

    #[tokio::test]
    async fn since_now_skips_backlog_but_follows_new_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("since-now.log");
        fs::write(&path, "old-a\nold-b\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let label = "since-now.log".to_string();
        let handle = tokio::spawn(watch_file(
            path.clone(),
            label,
            0,
            Duration::from_millis(10),
            tx,
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        fs::write(&path, "old-a\nold-b\nnew-c\n").unwrap();
        let item = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match rx.try_recv() {
                    Ok(item) => break item,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        tokio::task::yield_now().await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        panic!("watcher disconnected before emitting a line");
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(item.line, "new-c");

        handle.abort();
        let _ = handle.await;
    }
}
