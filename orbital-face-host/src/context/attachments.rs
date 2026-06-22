use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAttachment {
    pub title: String,
    pub content: String,
    pub source: String,
}

pub fn read_text_file(path: &Path) -> anyhow::Result<FileAttachment> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("attached file does not exist: {}", path.display()))?;
    anyhow::ensure!(metadata.is_file(), "attachment path is not a file");
    anyhow::ensure!(
        metadata.len() <= MAX_FILE_BYTES,
        "attached file exceeds the 64 KB limit"
    );

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read attached file {}", path.display()))?;
    anyhow::ensure!(
        !bytes.iter().take(8_192).any(|byte| *byte == 0),
        "attached file appears to be binary"
    );
    let content = String::from_utf8(bytes).context("attached file is not valid UTF-8 text")?;
    anyhow::ensure!(!content.trim().is_empty(), "attached file is empty");

    let source = path
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .display()
        .to_string();
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Attached File")
        .to_owned();
    Ok(FileAttachment {
        title,
        content,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_small_text_attachment() {
        let path = std::env::temp_dir().join(format!(
            "orbital-context-attachment-{}.txt",
            std::process::id()
        ));
        fs::write(&path, "hello attachment").unwrap();
        let attachment = read_text_file(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(attachment.content, "hello attachment");
    }

    #[test]
    fn missing_file_returns_clear_error() {
        let path = std::env::temp_dir().join("orbital-context-file-does-not-exist.txt");
        let error = read_text_file(&path).unwrap_err().to_string();
        assert!(error.contains("does not exist"));
    }

    #[test]
    fn binary_looking_file_is_rejected() {
        let path =
            std::env::temp_dir().join(format!("orbital-context-binary-{}.bin", std::process::id()));
        fs::write(&path, [1_u8, 0, 2, 3]).unwrap();
        let error = read_text_file(&path).unwrap_err().to_string();
        let _ = fs::remove_file(path);
        assert!(error.contains("binary"));
    }
}
