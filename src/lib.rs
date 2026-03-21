pub mod cli;
pub mod follow;
pub mod output;
pub mod resolve;
pub mod tail;

use anyhow::{Context, Result};
use std::sync::mpsc;

pub async fn run() -> Result<()> {
    let config = cli::Config::parse();
    let files = resolve::resolve_inputs(&config.inputs)
        .context("failed to resolve input paths/patterns")?;

    if files.is_empty() {
        anyhow::bail!("no files resolved from the provided arguments");
    }

    let labels = output::Labeler::new(&files);
    let (tx, rx) = mpsc::channel();

    let printer = std::thread::spawn(move || output::printer(rx, config.color));

    let mut handles = Vec::with_capacity(files.len());
    for path in files {
        let label = labels.label_for(&path);
        let tx = tx.clone();
        let lines = config.lines;
        handles.push(tokio::spawn(async move {
            follow::watch_file(path, label, lines, tx).await
        }));
    }
    drop(tx);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
    }

    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => eprintln!("cattail: worker error: {err:#}"),
            Err(err) if err.is_cancelled() => {}
            Err(err) => eprintln!("cattail: join error: {err}"),
        }
    }

    let _ = printer.join();
    Ok(())
}
