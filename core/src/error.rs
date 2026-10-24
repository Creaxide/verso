//! Typed errors with messages written for the person using the app.

use crate::format::Format;
use thiserror::Error;

/// Everything that can stop a conversion.
///
/// The `Display` text is the text the UI shows. Technical detail lives in the
/// variant fields so failures stay debuggable without leaking jargon.
#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("{from} → {to} is not supported. {reason}")]
    Unsupported {
        from: Format,
        to: Format,
        reason: &'static str,
    },

    #[error("This file does not look like {expected}. Its extension may be wrong.")]
    NotThisFormat { expected: Format },

    #[error("This PDF has no text layer — it looks like a scan. Run OCR first, then convert again.")]
    ScannedPdf,

    #[error("This PDF is password-protected or damaged and its text could not be read.")]
    PdfUnreadable,

    #[error("This image is {width} × {height} pixels. Verso handles images up to 10,000 px per side and 100 MP.")]
    ImageTooLarge { width: u32, height: u32 },

    #[error("PDF page rendering is unavailable. {detail}")]
    PdfRenderUnavailable { detail: String },

    #[error("A page could not be rendered: {detail}")]
    PdfRender { detail: String },

    #[error("The PDF could not be created.")]
    PdfWrite { detail: String },

    #[error("The {format} file could not be read.")]
    Parse { format: Format, detail: String },

    #[error("The {format} output could not be built.")]
    Encode { format: Format, detail: String },

    #[error("This document has no content to convert.")]
    EmptyDocument,

    #[error("This file is {size_mb} MB; Verso opens files up to {limit_mb} MB. Split it and convert the parts.")]
    TooLarge { size_mb: u64, limit_mb: u64 },

    #[error("The file could not be read or written.")]
    Io {
        #[source]
        source: std::io::Error,
    },

    #[error("Unexpected problem: {0}")]
    Other(String),
}

impl ConvertError {
    /// A short, stable code for logs and tests.
    pub fn code(&self) -> &'static str {
        match self {
            ConvertError::Unsupported { .. } => "unsupported-pair",
            ConvertError::NotThisFormat { .. } => "wrong-format",
            ConvertError::ScannedPdf => "scanned-pdf",
            ConvertError::PdfUnreadable => "pdf-unreadable",
            ConvertError::ImageTooLarge { .. } => "image-too-large",
            ConvertError::PdfRenderUnavailable { .. } => "pdf-render-unavailable",
            ConvertError::PdfRender { .. } => "pdf-render",
            ConvertError::PdfWrite { .. } => "pdf-write",
            ConvertError::Parse { .. } => "parse",
            ConvertError::Encode { .. } => "encode",
            ConvertError::EmptyDocument => "empty",
            ConvertError::TooLarge { .. } => "too-large",
            ConvertError::Io { .. } => "io",
            ConvertError::Other(_) => "other",
        }
    }

    /// Whether retrying the same input could plausibly help.
    pub fn is_retryable(&self) -> bool {
        matches!(self, ConvertError::Io { .. })
    }

    pub fn parse(format: Format, detail: impl std::fmt::Display) -> Self {
        ConvertError::Parse {
            format,
            detail: detail.to_string(),
        }
    }

    pub fn encode(format: Format, detail: impl std::fmt::Display) -> Self {
        ConvertError::Encode {
            format,
            detail: detail.to_string(),
        }
    }

    pub fn other(detail: impl std::fmt::Display) -> Self {
        ConvertError::Other(detail.to_string())
    }
}

impl From<std::io::Error> for ConvertError {
    fn from(source: std::io::Error) -> Self {
        ConvertError::Io { source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_like_sentences() {
        let error = ConvertError::Unsupported {
            from: Format::Pdf,
            to: Format::Csv,
            reason: "Table structure cannot be recovered.",
        };
        assert_eq!(
            error.to_string(),
            "PDF document → CSV table is not supported. Table structure cannot be recovered."
        );
        assert_eq!(error.code(), "unsupported-pair");
    }

    #[test]
    fn size_message_uses_megabytes() {
        let error = ConvertError::TooLarge {
            size_mb: 120,
            limit_mb: 64,
        };
        assert!(error.to_string().contains("120 MB"));
    }
}
