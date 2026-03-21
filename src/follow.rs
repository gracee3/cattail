use crate::output::OutputLine;
use crate::tail;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::time::{self, Duration};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

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

    pub fn reset_for_reopen(&mut self, len: u64) {
        self.offset = len;
        self.pending.clear();
    }

    fn ingest(&mut self, bytes: &[u8], flush_partial: bool) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();

        loop {
            let Some(pos) = self.pending.iter().position(|&b| b == b'\n') else {
                break;
            };
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

pub async fn watch_file(
    path: PathBuf,
    label: String,
    lines: usize,
    tx: mpsc::Sender<OutputLine>,
) -> Result<()> {
    let (backlog, len) = initial_backlog(&path, lines).await?;
    for line in backlog {
        let _ = tx.send(OutputLine {
            label: label.clone(),
            line,
        });
    }

    let mut state = FollowState::new(path, len);
    let mut tick = time::interval(POLL_INTERVAL);
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    loop {
        tick.tick().await;
        let _ = poll_once(&mut state, &label, &tx).await;
    }
}

async fn poll_once(
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
        state.reset_for_reopen(len);
        state.unavailable = false;
        return Ok(());
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
}
