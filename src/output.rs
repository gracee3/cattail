use crate::cli::ColorMode;
use anyhow::Result;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;

#[derive(Debug)]
pub struct OutputLine {
    pub label: String,
    pub line: String,
}

pub struct Labeler {
    labels: HashMap<PathBuf, String>,
}

impl Labeler {
    pub fn new(paths: &[PathBuf]) -> Self {
        let labels = unique_suffix_labels(paths);
        Self { labels }
    }

    pub fn label_for(&self, path: &Path) -> String {
        self.labels.get(path).cloned().unwrap_or_else(|| {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string()
        })
    }
}

pub fn printer(rx: mpsc::Receiver<OutputLine>, color: ColorMode) -> Result<()> {
    let use_color = match color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => std::io::stdout().is_terminal(),
    };

    let mut out = std::io::stdout();
    while let Ok(item) = rx.recv() {
        if use_color {
            writeln!(out, "\x1b[36m[{}]\x1b[0m {}", item.label, item.line)?;
        } else {
            writeln!(out, "[{}] {}", item.label, item.line)?;
        }
        out.flush()?;
    }
    Ok(())
}

fn unique_suffix_labels(paths: &[PathBuf]) -> HashMap<PathBuf, String> {
    let per_path: Vec<(PathBuf, Vec<String>)> = paths
        .iter()
        .cloned()
        .map(|path| {
            let mut parts = Vec::new();
            for component in path.components().rev() {
                if let Component::Normal(piece) = component {
                    parts.push(piece.to_string_lossy().to_string());
                }
            }
            if parts.is_empty() {
                parts.push(path.display().to_string());
            }
            (path, parts)
        })
        .collect();

    let mut width = vec![1usize; per_path.len()];
    loop {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, (_, parts)) in per_path.iter().enumerate() {
            let take = width[idx].min(parts.len());
            let label = parts
                .iter()
                .take(take)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("/");
            groups.entry(label).or_default().push(idx);
        }

        let mut changed = false;
        for indices in groups.values() {
            if indices.len() > 1 {
                for &idx in indices {
                    if width[idx] < per_path[idx].1.len() {
                        width[idx] += 1;
                        changed = true;
                    }
                }
            }
        }

        if !changed {
            let mut labels = HashMap::new();
            for (idx, (path, parts)) in per_path.into_iter().enumerate() {
                let take = width[idx].min(parts.len());
                let label = parts
                    .into_iter()
                    .take(take)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("/");
                labels.insert(path, label);
            }
            return labels;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn disambiguates_basenames() {
        let paths = vec![
            PathBuf::from("/var/log/orcas.log"),
            PathBuf::from("/tmp/orcas.log"),
        ];

        let labels = unique_suffix_labels(&paths);
        assert_ne!(labels[&paths[0]], labels[&paths[1]]);
        assert!(labels[&paths[0]].ends_with("orcas.log"));
        assert!(labels[&paths[1]].ends_with("orcas.log"));
    }
}
