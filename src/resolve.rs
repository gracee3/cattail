use anyhow::{Context, Result};
use glob::glob;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedFile {
    pub path: PathBuf,
}

pub fn resolve_inputs(inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for input in inputs {
        let mut matches = expand_input(input)?;
        matches.sort();
        for path in matches {
            let key = dedupe_key(&path)?;
            if seen.insert(key) {
                resolved.push(path);
            }
        }
    }

    resolved.sort();
    Ok(resolved)
}

fn expand_input(input: &str) -> Result<Vec<PathBuf>> {
    if has_glob_magic(input) {
        let mut matches = Vec::new();
        for entry in glob(input).with_context(|| format!("invalid glob pattern: {input}"))? {
            match entry {
                Ok(path) if path.exists() => matches.push(path),
                Ok(_) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(matches)
    } else {
        let path = PathBuf::from(input);
        if path.exists() {
            Ok(vec![path])
        } else {
            Ok(Vec::new())
        }
    }
}

fn has_glob_magic(input: &str) -> bool {
    input.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

fn dedupe_key(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        Ok(std::fs::canonicalize(path).with_context(|| format!("canonicalizing {path:?}"))?)
    } else {
        Ok(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_and_dedupes_globs_and_literals() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.log");
        let b = dir.path().join("b.log");
        let c = dir.path().join("c.txt");
        fs::write(&a, "x\n").unwrap();
        fs::write(&b, "y\n").unwrap();
        fs::write(&c, "z\n").unwrap();

        let inputs = vec![
            format!("{}/{}.log", dir.path().display(), "*"),
            a.display().to_string(),
        ];

        let resolved = resolve_inputs(&inputs).unwrap();
        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains(&a));
        assert!(resolved.contains(&b));
        assert!(!resolved.contains(&c));
    }
}
