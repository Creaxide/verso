//! Format identity and detection.

use std::fmt;
use std::path::Path;

/// Every format Verso can read or write.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Txt,
    Markdown,
    Html,
    Docx,
    Pdf,
    Rtf,
    Json,
    Csv,
}

impl Format {
    pub const ALL: [Format; 8] = [
        Format::Txt,
        Format::Markdown,
        Format::Html,
        Format::Docx,
        Format::Pdf,
        Format::Rtf,
        Format::Json,
        Format::Csv,
    ];

    /// Short uppercase code used in the UI (`TXT`, `MD`, …).
    pub const fn code(self) -> &'static str {
        match self {
            Format::Txt => "TXT",
            Format::Markdown => "MD",
            Format::Html => "HTML",
            Format::Docx => "DOCX",
            Format::Pdf => "PDF",
            Format::Rtf => "RTF",
            Format::Json => "JSON",
            Format::Csv => "CSV",
        }
    }

    /// Human name shown next to the code.
    pub const fn name(self) -> &'static str {
        match self {
            Format::Txt => "Plain text",
            Format::Markdown => "Markdown",
            Format::Html => "Web page",
            Format::Docx => "Word document",
            Format::Pdf => "PDF document",
            Format::Rtf => "Rich text",
            Format::Json => "JSON",
            Format::Csv => "CSV table",
        }
    }

    /// Lowercase extension, without a dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Format::Txt => "txt",
            Format::Markdown => "md",
            Format::Html => "html",
            Format::Docx => "docx",
            Format::Pdf => "pdf",
            Format::Rtf => "rtf",
            Format::Json => "json",
            Format::Csv => "csv",
        }
    }

    pub const fn mime(self) -> &'static str {
        match self {
            Format::Txt => "text/plain",
            Format::Markdown => "text/markdown",
            Format::Html => "text/html",
            Format::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Format::Pdf => "application/pdf",
            Format::Rtf => "application/rtf",
            Format::Json => "application/json",
            Format::Csv => "text/csv",
        }
    }

    /// Formats whose bytes are not reasonably renderable as text.
    pub const fn is_binary(self) -> bool {
        matches!(self, Format::Docx | Format::Pdf)
    }

    /// A one-line description of what the format is for, shown in the UI.
    pub const fn description(self) -> &'static str {
        match self {
            Format::Txt => "Unformatted text. Structure is carried by blank lines and line breaks.",
            Format::Markdown => "Lightweight markup with headings, lists, links and tables.",
            Format::Html => "A standalone web page with its styles embedded.",
            Format::Docx => "Microsoft Word format: headings, lists, tables and links.",
            Format::Pdf => "Fixed-page document. Text is reflowed; layout is not reproduced.",
            Format::Rtf => "Rich text with character formatting; no document structure.",
            Format::Json => "Structured export. A lone table becomes an array of objects.",
            Format::Csv => "Delimited table. The first table in the document is exported.",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Format> {
        // `match` on a lowercased string needs allocation; the caller passes a
        // lowercased extension, and we accept the handful of aliases here.
        match ext {
            "txt" | "text" => Some(Format::Txt),
            "md" | "markdown" | "mdown" | "mkd" => Some(Format::Markdown),
            "html" | "htm" | "xhtml" => Some(Format::Html),
            "docx" => Some(Format::Docx),
            "pdf" => Some(Format::Pdf),
            "rtf" => Some(Format::Rtf),
            "json" => Some(Format::Json),
            "csv" | "tsv" => Some(Format::Csv),
            _ => None,
        }
    }

    /// Extension of a path, lowercased, without the dot.
    pub fn extension_of(path: &Path) -> Option<String> {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
    }

    /// Best-effort detection for files with no useful extension.
    ///
    /// Content sniffing only recognizes self-identifying formats; anything else
    /// falls back to plain text. The extension always wins when it is known,
    /// because users rename files on purpose.
    pub fn sniff(bytes: &[u8]) -> Format {
        let head = &bytes[..bytes.len().min(4096)];

        if head.starts_with(b"%PDF-") {
            return Format::Pdf;
        }
        if head.starts_with(b"{\\rtf") {
            return Format::Rtf;
        }
        if head.starts_with(b"PK\x03\x04")
            && head
                .windows(5)
                .any(|window| window == b"word/" || window == b"word\\")
        {
            return Format::Docx;
        }

        let text = String::from_utf8_lossy(head);
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            return Format::Html;
        }
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                return Format::Json;
            }
        }
        if looks_like_csv(&text) {
            return Format::Csv;
        }
        Format::Txt
    }

    /// Extension + sniffing, used when the user has not pinned a source format.
    pub fn detect(path: Option<&Path>, bytes: &[u8]) -> Format {
        if let Some(path) = path {
            if let Some(ext) = Format::extension_of(path) {
                if let Some(format) = Format::from_extension(&ext) {
                    return format;
                }
            }
        }
        Format::sniff(bytes)
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Format {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Format::from_extension(&value.to_ascii_lowercase()).ok_or(())
    }
}

/// Heuristic used only by [`Format::sniff`]: at least two non-empty lines with
/// the same count of one delimiter.
fn looks_like_csv(text: &str) -> bool {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    let Some(second) = lines.next() else {
        return false;
    };
    for delimiter in [',', ';', '\t', '|'] {
        let a = first.matches(delimiter).count();
        let b = second.matches(delimiter).count();
        if a > 0 && a == b {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extensions_round_trip() {
        for format in Format::ALL {
            assert_eq!(Format::from_extension(format.extension()), Some(format));
        }
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(Format::from_extension("markdown"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("htm"), Some(Format::Html));
        assert_eq!(Format::from_extension("nope"), None);
    }

    #[test]
    fn sniff_identifies_self_describing_files() {
        assert_eq!(Format::sniff(b"%PDF-1.7\n"), Format::Pdf);
        assert_eq!(Format::sniff(b"{\\rtf1\\ansi hello}"), Format::Rtf);
        assert_eq!(Format::sniff(b"<!doctype html><html></html>"), Format::Html);
        assert_eq!(Format::sniff(br#"{"a": 1}"#), Format::Json);
        assert_eq!(Format::sniff(b"a,b,c\n1,2,3\n"), Format::Csv);
        assert_eq!(Format::sniff(b"just words"), Format::Txt);
    }

    #[test]
    fn extension_wins_over_content() {
        let path = PathBuf::from("notes.txt");
        assert_eq!(Format::detect(Some(&path), b"%PDF-1.7"), Format::Txt);
    }

    #[test]
    fn unknown_extension_falls_back_to_sniffing() {
        let path = PathBuf::from("report.weird");
        assert_eq!(Format::detect(Some(&path), b"%PDF-1.7"), Format::Pdf);
    }
}
