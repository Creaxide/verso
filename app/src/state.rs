//! Application state. No egui types live here beyond colours of meaning.

use std::path::{Path, PathBuf};

use verso_core::{DocumentSummary, Format};

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

#[derive(Default)]
pub enum SourceState {
    #[default]
    Empty,
    Loading {
        path: PathBuf,
        format: Option<Format>,
    },
    File(SourceFile),
    Batch(Batch),
}

impl SourceState {
    pub fn is_empty(&self) -> bool {
        matches!(self, SourceState::Empty)
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, SourceState::Loading { .. })
    }

    pub fn is_batch(&self) -> bool {
        matches!(self, SourceState::Batch(_))
    }

    pub fn count(&self) -> usize {
        match self {
            SourceState::Empty => 0,
            SourceState::Loading { .. } => 1,
            SourceState::File(_) => 1,
            SourceState::Batch(batch) => batch.files.len(),
        }
    }

    /// The format read for a single file. `None` for a batch with mixed formats.
    pub fn format(&self) -> Option<Format> {
        match self {
            SourceState::File(file) => Some(file.format),
            SourceState::Loading { format, .. } => *format,
            _ => None,
        }
    }

    pub fn batch(&self) -> Option<&Batch> {
        match self {
            SourceState::Batch(batch) => Some(batch),
            _ => None,
        }
    }

    pub fn file(&self) -> Option<&SourceFile> {
        match self {
            SourceState::File(file) => Some(file),
            _ => None,
        }
    }

    pub fn primary_path(&self) -> Option<&Path> {
        match self {
            SourceState::File(file) => Some(&file.path),
            SourceState::Loading { path, .. } => Some(path),
            SourceState::Batch(batch) => batch.files.first().map(|file| file.path.as_path()),
            SourceState::Empty => None,
        }
    }
}

pub struct SourceFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub format: Format,
    pub size: u64,
    pub inspection: Inspection,
}

impl SourceFile {
    pub fn display_name(&self) -> String {
        file_name(&self.path)
    }
}

#[derive(Default)]
pub enum Inspection {
    #[default]
    Pending,
    Ready(Preview),
    Failed(String),
}

#[derive(Clone, Default)]
pub struct Preview {
    pub summary: DocumentSummary,
    pub snippet: String,
    pub truncated: bool,
}

pub struct Batch {
    pub files: Vec<BatchFile>,
}

impl Batch {
    pub fn new(files: Vec<BatchFile>) -> Self {
        Self { files }
    }

    pub fn waiting(&self) -> usize {
        self.files
            .iter()
            .filter(|file| {
                matches!(
                    file.status,
                    BatchStatus::Waiting | BatchStatus::Failed(_) | BatchStatus::Skipped(_)
                )
            })
            .count()
    }

    pub fn done(&self) -> usize {
        self.files
            .iter()
            .filter(|file| matches!(file.status, BatchStatus::Done { .. }))
            .count()
    }

    pub fn reset(&mut self) {
        for file in &mut self.files {
            file.status = BatchStatus::Waiting;
        }
    }

    pub fn reset_running(&mut self) {
        for file in &mut self.files {
            if matches!(file.status, BatchStatus::Running) {
                file.status = BatchStatus::Waiting;
            }
        }
    }
}

pub struct BatchFile {
    pub path: PathBuf,
    pub format: Format,
    pub size: u64,
    pub status: BatchStatus,
}

impl BatchFile {
    pub fn display_name(&self) -> String {
        file_name(&self.path)
    }
}

#[derive(Clone, Debug, Default)]
pub enum BatchStatus {
    #[default]
    Waiting,
    Running,
    Done {
        output: PathBuf,
        bytes: usize,
        warnings: Vec<String>,
        elapsed_ms: u128,
    },
    Failed(String),
    /// A supported target exists, but not for this file's source format.
    Skipped(String),
}

impl BatchStatus {
    pub fn label(&self) -> &'static str {
        match self {
            BatchStatus::Waiting => "WAITING",
            BatchStatus::Running => "EXPOSING",
            BatchStatus::Done { .. } => "DONE",
            BatchStatus::Failed(_) => "FAILED",
            BatchStatus::Skipped(_) => "SKIPPED",
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            BatchStatus::Failed(message) | BatchStatus::Skipped(message) => Some(message),
            _ => None,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self, BatchStatus::Done { .. })
    }
}

/// The result of a batch run, kept for the proof panel.
#[derive(Clone, Debug, Default)]
pub struct BatchSummary {
    pub done: usize,
    pub failed: usize,
    pub skipped: usize,
    pub bytes: u64,
    pub warnings: usize,
    pub elapsed_ms: u128,
    pub folder: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Completed {
    pub output: PathBuf,
    pub source_format: Format,
    pub output_format: Format,
    pub bytes: usize,
    pub warnings: Vec<String>,
    pub snippet: String,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

impl Completed {
    pub fn output_name(&self) -> String {
        file_name(&self.output)
    }

    pub fn folder(&self) -> PathBuf {
        self.output
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub enum Notice {
    #[default]
    Idle,
    Info(String),
    Success(String),
    Error(String),
}

impl Notice {
    pub fn text(&self) -> &str {
        match self {
            Notice::Idle => "Ready",
            Notice::Info(message) | Notice::Success(message) | Notice::Error(message) => message,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Notice::Error(_))
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

/// `12.4 KB`, with one decimal where it helps.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Keep the end of a long path, where the filename lives.
pub fn shorten_path(path: &Path, limit: usize) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= limit {
        return text;
    }
    let tail: String = text
        .chars()
        .rev()
        .take(limit.saturating_sub(2))
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{tail}")
}

/// The target we fall back to when the current one cannot read this source.
pub fn fallback_target(source: Format) -> Format {
    for candidate in [
        Format::Markdown,
        Format::Txt,
        Format::Html,
        Format::Docx,
        Format::Pdf,
        Format::Rtf,
        Format::Json,
        Format::Csv,
    ] {
        if verso_core::is_supported(source, candidate) {
            return candidate;
        }
    }
    Format::Txt
}
