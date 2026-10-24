//! Output file placement.
//!
//! Two rules, both non-negotiable:
//! 1. Never write over the source file, even when converting TXT → TXT.
//! 2. Never write over an existing file; add `_1`, `_2`, … instead.
//!
//! [`write_output`] enforces both atomically with `create_new`, so two app
//! instances converting the same file cannot race into each other.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::error::ConvertError;
use crate::format::Format;

/// How many numbered names to try before giving up.
const MAX_ATTEMPTS: u32 = 10_000;

/// `report.txt` → `<same folder>/report.md`. Purely a path computation.
pub fn sibling_output(input: &Path, format: Format) -> PathBuf {
    let (parent, stem) = split_input(input);
    parent.join(format!("{stem}.{}", format.extension()))
}

/// The first free `_N` name next to the input. A hint for the UI; the writer
/// still reserves the name with [`write_output`] so nothing is overwritten.
pub fn unique_output(input: &Path, format: Format) -> PathBuf {
    let (parent, stem) = split_input(input);
    for attempt in 0..MAX_ATTEMPTS {
        let candidate = candidate_path(&parent, &stem, format, attempt);
        if !candidate.exists() {
            return candidate;
        }
    }
    numbered_fallback(&parent, &stem, format)
}

/// Write `bytes` to the first free sibling name and return the path used.
///
/// The source file is never touched, and an existing file is never truncated.
pub fn write_output(
    input: &Path,
    format: Format,
    bytes: &[u8],
) -> Result<PathBuf, ConvertError> {
    let (parent, stem) = split_input(input);

    for attempt in 0..MAX_ATTEMPTS {
        let path = candidate_path(&parent, &stem, format, attempt);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|()| file.flush()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(ConvertError::Io { source: error });
                }
                return Ok(path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ConvertError::Io { source: error }),
        }
    }

    Err(ConvertError::other(
        "Could not find a free file name next to the source file.",
    ))
}

fn split_input(input: &Path) -> (PathBuf, String) {
    let parent = match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("converted")
        .to_string();
    (parent, stem)
}

fn candidate_path(parent: &Path, stem: &str, format: Format, attempt: u32) -> PathBuf {
    let extension = format.extension();
    if attempt == 0 {
        parent.join(format!("{stem}.{extension}"))
    } else {
        parent.join(format!("{stem}_{attempt}.{extension}"))
    }
}

/// If ten thousand numbered names are taken, use a timestamp rather than fail
/// on a perfect storm.
fn numbered_fallback(parent: &Path, stem: &str, format: Format) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!("{stem}_{stamp}.{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn scratch(name: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "verso-paths-{}-{}-{}",
            std::process::id(),
            name,
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn sibling_keeps_folder_and_changes_extension() {
        let path = PathBuf::from("C:/work/notes/report.txt");
        assert_eq!(
            sibling_output(&path, Format::Markdown),
            PathBuf::from("C:/work/notes/report.md")
        );
    }

    #[test]
    fn never_overwrites_an_existing_file() {
        let dir = scratch("collision");
        let input = dir.join("report.txt");
        fs::write(&input, b"source").expect("write input");
        fs::write(dir.join("report.md"), b"already here").expect("write existing");

        let written = write_output(&input, Format::Markdown, b"new").expect("writes");
        assert_eq!(written.file_name().and_then(|n| n.to_str()), Some("report_1.md"));
        assert_eq!(fs::read(&written).expect("read output"), b"new");
        assert_eq!(fs::read(&input).expect("read input"), b"source");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_format_conversion_does_not_replace_the_source() {
        let dir = scratch("same");
        let input = dir.join("notes.txt");
        fs::write(&input, b"original").expect("write input");

        let written = write_output(&input, Format::Txt, b"normalized").expect("writes");
        assert_eq!(written.file_name().and_then(|n| n.to_str()), Some("notes_1.txt"));
        assert_eq!(fs::read(&input).expect("read input"), b"original");
        assert_eq!(fs::read(&written).expect("read output"), b"normalized");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_output_reports_the_name_the_writer_would_use() {
        let dir = scratch("hint");
        let input = dir.join("table.csv");
        fs::write(&input, b"a,b\n").expect("write input");
        fs::write(dir.join("table.json"), b"{}").expect("write existing");

        assert_eq!(
            unique_output(&input, Format::Json)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("table_1.json")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
