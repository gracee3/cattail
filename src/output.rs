use crate::cli::ColorMode;
use crate::cli::PrefixMode;
use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;

#[derive(Debug)]
pub struct OutputLine {
    pub label: String,
    pub line: String,
}

pub struct LabelRegistry {
    prefix: PrefixMode,
    cwd: PathBuf,
    labels: HashMap<PathBuf, String>,
    used: HashSet<String>,
}

impl LabelRegistry {
    pub fn new(prefix: PrefixMode) -> Self {
        Self {
            prefix,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            labels: HashMap::new(),
            used: HashSet::new(),
        }
    }

    pub fn with_paths(paths: &[PathBuf], prefix: PrefixMode) -> Self {
        let mut registry = Self::new(prefix);
        for path in paths {
            let _ = registry.allocate(path);
        }
        registry
    }

    pub fn allocate(&mut self, path: &Path) -> String {
        if let Some(label) = self.labels.get(path) {
            return label.clone();
        }

        let label = match self.prefix {
            PrefixMode::Basename => allocate_basename(path, &self.used),
            PrefixMode::Relative => {
                let candidate = relative_display(path, &self.cwd);
                if self.used.insert(candidate.clone()) {
                    candidate
                } else {
                    absolute_display(path, &self.cwd)
                }
            }
            PrefixMode::Full => absolute_display(path, &self.cwd),
        };

        self.used.insert(label.clone());
        self.labels.insert(path.to_path_buf(), label.clone());
        label
    }

    pub fn label_for(&self, path: &Path) -> Option<&str> {
        self.labels.get(path).map(String::as_str)
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

fn allocate_basename(path: &Path, used: &HashSet<String>) -> String {
    for candidate in basename_candidates(path) {
        if !used.contains(&candidate) {
            return candidate;
        }
    }

    path.display().to_string()
}

fn basename_candidates(path: &Path) -> Vec<String> {
    let mut components = Vec::new();
    for component in path.components() {
        if let Component::Normal(piece) = component {
            components.push(piece.to_string_lossy().to_string());
        }
    }

    if components.is_empty() {
        return vec![path.display().to_string()];
    }

    let mut candidates = Vec::with_capacity(components.len());
    for start in (0..components.len()).rev() {
        candidates.push(components[start..].join("/"));
    }
    candidates
}

fn relative_display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|rel| rel.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn absolute_display(path: &Path, cwd: &Path) -> String {
    if path.is_absolute() {
        path.display().to_string()
    } else {
        cwd.join(path).display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn disambiguates_basenames() {
        let paths = vec![
            PathBuf::from("/var/log/orcas.log"),
            PathBuf::from("/tmp/orcas.log"),
        ];

        let labels = {
            let registry = LabelRegistry::with_paths(&paths, PrefixMode::Basename);
            vec![
                registry.label_for(&paths[0]).unwrap().to_string(),
                registry.label_for(&paths[1]).unwrap().to_string(),
            ]
        };
        assert_ne!(labels[0], labels[1]);
        assert!(labels[0].ends_with("orcas.log"));
        assert!(labels[1].ends_with("orcas.log"));
    }

    #[test]
    fn relative_prefix_uses_cwd_when_possible() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().join("cwd");
        let logs = cwd.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let path = logs.join("app.log");
        std::fs::write(&path, "x\n").unwrap();

        let mut registry = LabelRegistry::new(PrefixMode::Relative);
        registry.cwd = cwd;
        let label = registry.allocate(&path);
        assert_eq!(label, "logs/app.log");
    }

    #[test]
    fn full_prefix_uses_absolute_path() {
        let cwd = PathBuf::from("/tmp/cattail-test");
        let path = PathBuf::from("logs/app.log");
        let label = absolute_display(&path, &cwd);
        assert_eq!(label, "/tmp/cattail-test/logs/app.log");
    }

    #[test]
    fn label_registry_honors_selected_prefix_mode() {
        let paths = vec![PathBuf::from("/var/log/app.log")];
        let labeler = LabelRegistry::with_paths(&paths, PrefixMode::Full);
        assert_eq!(labeler.label_for(&paths[0]).unwrap(), "/var/log/app.log");
    }

    #[test]
    fn incremental_allocation_keeps_existing_labels_stable() {
        let mut registry = LabelRegistry::new(PrefixMode::Basename);
        let first = registry.allocate(Path::new("/var/log/app.log"));
        let second = registry.allocate(Path::new("/tmp/app.log"));
        assert_ne!(first, second);
        assert_eq!(
            registry.label_for(Path::new("/var/log/app.log")).unwrap(),
            first
        );
        assert_eq!(
            registry.label_for(Path::new("/tmp/app.log")).unwrap(),
            second
        );
    }
}
