use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_SIZE: usize = 8 * 1024;

pub fn read_last_lines(path: &Path, lines: usize) -> Result<Vec<String>> {
    if lines == 0 {
        return Ok(Vec::new());
    }

    let mut file = File::open(path).with_context(|| format!("opening {path:?}"))?;
    let len = file.metadata()?.len();
    let mut offset = len;
    let mut chunks = Vec::new();
    let mut newline_count = 0usize;

    while offset > 0 && newline_count <= lines {
        let size = usize::min(CHUNK_SIZE, offset as usize);
        offset -= size as u64;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size];
        file.read_exact(&mut buf)?;
        newline_count += buf.iter().filter(|&&b| b == b'\n').count();
        chunks.push(buf);
    }

    chunks.reverse();
    let mut data = chunks.into_iter().flatten().collect::<Vec<u8>>();
    if offset > 0 {
        let mut prev = [0u8; 1];
        file.seek(SeekFrom::Start(offset - 1))?;
        file.read_exact(&mut prev)?;
        if prev[0] != b'\n' {
            if let Some(pos) = data.iter().position(|&b| b == b'\n') {
                data.drain(..=pos);
            }
        }
    }

    Ok(split_lines(&data, true)
        .into_iter()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

pub fn split_lines(bytes: &[u8], include_final_partial: bool) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = Vec::new();
    for piece in text.split_terminator('\n') {
        lines.push(piece.trim_end_matches('\r').to_string());
    }
    if include_final_partial && text.is_empty() && bytes.is_empty() {
        return lines;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn extracts_last_n_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tail.log");
        fs::write(&path, "a\nb\nc\nd\n").unwrap();

        let lines = read_last_lines(&path, 2).unwrap();
        assert_eq!(lines, vec!["c".to_string(), "d".to_string()]);
    }

    #[test]
    fn includes_final_partial_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.log");
        fs::write(&path, "a\nb\nc").unwrap();

        let lines = read_last_lines(&path, 2).unwrap();
        assert_eq!(lines, vec!["b".to_string(), "c".to_string()]);
    }
}
