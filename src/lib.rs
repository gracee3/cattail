//! `cattail` tails multiple files and glob patterns with live discovery.

pub mod cli;
pub mod follow;
pub mod output;
pub mod resolve;
pub mod tail;
pub mod watch;

use anyhow::{Context, Result};
use std::sync::mpsc;

pub async fn run() -> Result<()> {
    let config = cli::Config::parse();
    let (tx, rx) = mpsc::channel();

    let printer = std::thread::spawn(move || output::printer(rx, config.color));

    let runtime = watch::start(config, tx).context("failed to start file watcher")?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
    }

    let _ = runtime.shutdown();
    let _ = printer.join();
    Ok(())
}
