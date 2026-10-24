//! Conversion options, held by the app and passed to every converter.

/// Physical page sizes PDF output understands.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PageSize {
    A4,
    Letter,
    Legal,
}

impl PageSize {
    pub const ALL: [PageSize; 3] = [PageSize::A4, PageSize::Letter, PageSize::Legal];

    /// Width and height in PostScript points.
    pub const fn points(self) -> (f32, f32) {
        match self {
            // 210 × 297 mm
            PageSize::A4 => (595.28, 841.89),
            // 8.5 × 11 in
            PageSize::Letter => (612.0, 792.0),
            // 8.5 × 14 in
            PageSize::Legal => (612.0, 1008.0),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::Letter => "Letter",
            PageSize::Legal => "Legal",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfOptions {
    pub page_size: PageSize,
    pub margin_pt: f32,
    pub body_pt: f32,
    pub show_page_numbers: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::A4,
            margin_pt: 56.0,
            body_pt: 10.5,
            show_page_numbers: true,
        }
    }
}

/// Knobs the UI exposes. Everything has a defensible default so the library
/// can be used without any UI at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub pdf: PdfOptions,
    /// Single byte delimiter for CSV output.
    pub csv_delimiter: u8,
    /// Refuse inputs larger than this before parsing.
    pub max_input_bytes: u64,
    /// Pretty-print JSON output.
    pub json_pretty: bool,
    /// Inline image bytes up to this size in JSON output; larger images are
    /// represented by their alt text only.
    pub image_data_limit: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            pdf: PdfOptions::default(),
            csv_delimiter: b',',
            max_input_bytes: 64 * 1024 * 1024,
            json_pretty: true,
            image_data_limit: 4 * 1024 * 1024,
        }
    }
}

impl Options {
    pub fn with_delimiter(mut self, delimiter: u8) -> Self {
        self.csv_delimiter = delimiter;
        self
    }

    pub fn with_page_size(mut self, page_size: PageSize) -> Self {
        self.pdf.page_size = page_size;
        self
    }

    pub fn check_size(&self, bytes: &[u8]) -> Result<(), crate::ConvertError> {
        if bytes.len() as u64 > self.max_input_bytes {
            return Err(crate::ConvertError::TooLarge {
                size_mb: bytes.len() as u64 / (1024 * 1024),
                limit_mb: self.max_input_bytes / (1024 * 1024),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_sizes_are_portrait_points() {
        let (w, h) = PageSize::A4.points();
        assert!(w < h);
        assert!((w - 595.28).abs() < 0.01);
    }

    #[test]
    fn size_limit_is_enforced() {
        let options = Options {
            max_input_bytes: 4,
            ..Options::default()
        };
        assert!(options.check_size(b"123").is_ok());
        assert!(options.check_size(b"12345").is_err());
    }
}
