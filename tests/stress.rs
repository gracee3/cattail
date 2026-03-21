use cattail::cli::{ColorMode, Config, PrefixMode};
use cattail::output::OutputLine;
use cattail::watch;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc as tokio_mpsc;

struct Harness {
    runtime: Option<watch::WatchRuntime>,
    rx: tokio_mpsc::UnboundedReceiver<OutputLine>,
}

impl Harness {
    fn start(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let runtime = watch::start(config, tx).unwrap();
        let rx = forward_receiver(rx);
        Self {
            runtime: Some(runtime),
            rx,
        }
    }

    fn shutdown(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown().unwrap();
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = runtime.shutdown();
        }
    }
}

fn config(inputs: Vec<String>, since_now: bool, interval_ms: u64) -> Config {
    Config {
        lines: 50,
        interval_ms,
        prefix: PrefixMode::Basename,
        since_now,
        color: ColorMode::Never,
        inputs,
    }
}

fn forward_receiver(rx: mpsc::Receiver<OutputLine>) -> tokio_mpsc::UnboundedReceiver<OutputLine> {
    let (tx, out_rx) = tokio_mpsc::unbounded_channel();
    thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            let _ = tx.send(item);
        }
    });
    out_rx
}

async fn receive_n(
    rx: &mut tokio_mpsc::UnboundedReceiver<OutputLine>,
    n: usize,
) -> Vec<OutputLine> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for output")
            .expect("output stream closed early");
        out.push(line);
    }
    out
}

async fn assert_quiet(rx: &mut tokio_mpsc::UnboundedReceiver<OutputLine>, wait: Duration) {
    tokio::time::sleep(wait).await;
    assert!(rx.try_recv().is_err(), "unexpected extra output");
}

fn write_lines(path: &Path, lines: &[&str]) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}

#[tokio::test]
async fn repeated_modify_bursts_emit_each_line_once() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("burst.log");
    fs::write(&file, "").unwrap();

    let mut harness = Harness::start(config(vec![file.display().to_string()], true, 10));

    tokio::time::sleep(Duration::from_millis(100)).await;
    for idx in 0..10 {
        write_lines(&file, &[&format!("burst-{idx}")]);
    }

    let lines = receive_n(&mut harness.rx, 10).await;
    let payloads: Vec<_> = lines.iter().map(|line| line.line.as_str()).collect();
    assert_eq!(
        payloads,
        vec![
            "burst-0", "burst-1", "burst-2", "burst-3", "burst-4", "burst-5", "burst-6", "burst-7",
            "burst-8", "burst-9",
        ]
    );
    assert!(lines.iter().all(|line| line.label == "burst.log"));
    assert_quiet(&mut harness.rx, Duration::from_millis(250)).await;

    harness.shutdown();
}

#[tokio::test]
async fn overlapping_globs_do_not_duplicate_following() {
    let dir = tempdir().unwrap();
    let logs = dir.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let file = logs.join("worker.log");
    fs::write(&file, "seed\n").unwrap();

    let mut harness = Harness::start(config(
        vec![
            format!("{}/{}.log", logs.display(), "*"),
            format!("{}/or*.log", logs.display()),
        ],
        true,
        10,
    ));

    tokio::time::sleep(Duration::from_millis(100)).await;
    write_lines(&file, &["live-1", "live-2"]);
    let lines = receive_n(&mut harness.rx, 2).await;
    let payloads: Vec<_> = lines.iter().map(|line| line.line.as_str()).collect();
    assert_eq!(payloads, vec!["live-1", "live-2"]);
    assert!(lines.iter().all(|line| line.label == "worker.log"));
    assert_quiet(&mut harness.rx, Duration::from_millis(250)).await;

    harness.shutdown();
}

#[tokio::test]
async fn truncate_then_append_stays_single_stream() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("truncate.log");
    fs::write(&file, "seed\n").unwrap();

    let mut harness = Harness::start(config(vec![file.display().to_string()], false, 10));

    let backlog = receive_n(&mut harness.rx, 1).await;
    assert_eq!(backlog[0].line, "seed");

    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&file)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    write_lines(&file, &["after-truncate"]);
    let lines = receive_n(&mut harness.rx, 1).await;
    assert_eq!(lines[0].line, "after-truncate");
    assert_quiet(&mut harness.rx, Duration::from_millis(250)).await;

    harness.shutdown();
}

#[tokio::test]
async fn recreate_then_rapid_writes_emit_once_each() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("recreate.log");
    fs::write(&file, "seed\n").unwrap();

    let mut harness = Harness::start(config(vec![file.display().to_string()], true, 10));

    tokio::time::sleep(Duration::from_millis(100)).await;
    fs::remove_file(&file).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::write(&file, "fresh-1\nfresh-2\n").unwrap();
    write_lines(&file, &["fresh-3"]);

    let lines = receive_n(&mut harness.rx, 3).await;
    let payloads: Vec<_> = lines.iter().map(|line| line.line.as_str()).collect();
    assert_eq!(payloads, vec!["fresh-1", "fresh-2", "fresh-3"]);
    assert_quiet(&mut harness.rx, Duration::from_millis(250)).await;

    harness.shutdown();
}

#[tokio::test]
async fn new_matching_file_is_discovered_then_followed_once() {
    let dir = tempdir().unwrap();
    let logs = dir.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let pattern = format!("{}/{}.log", logs.display(), "*");

    let mut harness = Harness::start(config(vec![pattern], true, 10));

    tokio::time::sleep(Duration::from_millis(100)).await;
    let file = logs.join("dynamic.log");
    fs::write(&file, "boot\nready\n").unwrap();

    let lines = receive_n(&mut harness.rx, 2).await;
    let payloads: Vec<_> = lines.iter().map(|line| line.line.as_str()).collect();
    assert_eq!(payloads, vec!["boot", "ready"]);
    assert!(lines.iter().all(|line| line.label == "dynamic.log"));

    write_lines(&file, &["live-1", "live-2"]);
    let more = receive_n(&mut harness.rx, 2).await;
    let payloads: Vec<_> = more.iter().map(|line| line.line.as_str()).collect();
    assert_eq!(payloads, vec!["live-1", "live-2"]);
    assert_quiet(&mut harness.rx, Duration::from_millis(300)).await;

    harness.shutdown();
}
