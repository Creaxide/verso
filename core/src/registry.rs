//! The registry: one [`Converter`] per supported format pair.
//!
//! Every pair shares the same shape — read into the document model, write out
//! again — so a single [`IrConverter`] type covers all of them while the
//! registry keeps the "one implementation per pair" contract visible. Adding a
//! format means adding a reader, a writer and its row in [`crate::Format::ALL`].

use std::sync::OnceLock;

use crate::document::{Document, DocumentSummary};
use crate::error::ConvertError;
use crate::format::Format;
use crate::matrix::{is_supported, pair_info};
use crate::options::Options;
use crate::readers;
use crate::readers::image::ImageConverter;
use crate::readers::image_pdf::ImageToPdfConverter;
use crate::readers::pdf_image::{default_renderer, PdfToImageConverter};
use crate::traits::{Reader, Writer};

/// One file produced by a conversion. `name` is a relative file name; the app
/// decides which folder it lands in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A conversion between two concrete formats.
///
/// The signature is deliberately the whole contract: bytes in, files out, no
/// file system access, no UI, no hidden state.
pub trait Converter: Send + Sync {
    fn source(&self) -> Format;
    fn target(&self) -> Format;
    fn convert(&self, input: &[u8], opts: &Options) -> Result<Vec<OutputFile>, ConvertError>;

    /// Convert, reporting work as it happens. `progress(done, total)` is called
    /// at least once; multi-page renderers call it once per page.
    fn convert_with_progress(
        &self,
        input: &[u8],
        opts: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<OutputFile>, ConvertError> {
        let files = self.convert(input, opts)?;
        let total = files.len().max(1);
        progress(total, total);
        Ok(files)
    }

    /// Convert and keep warnings and source statistics. Converters that do not
    /// need a document model can leave the default in place.
    fn run(
        &self,
        input: &[u8],
        opts: &Options,
    ) -> Result<ConversionReport, ConvertError> {
        let files = self.convert(input, opts)?;
        Ok(ConversionReport {
            files,
            warnings: Vec::new(),
            summary: DocumentSummary::default(),
            from: self.source(),
            to: self.target(),
        })
    }

    /// Like [`Converter::run`], but reporting per-page progress.
    fn run_with_progress(
        &self,
        input: &[u8],
        opts: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ConversionReport, ConvertError> {
        let files = self.convert_with_progress(input, opts, progress)?;
        Ok(ConversionReport {
            files,
            warnings: Vec::new(),
            summary: DocumentSummary::default(),
            from: self.source(),
            to: self.target(),
        })
    }
}

/// The one converter implementation shared by every supported pair.
#[derive(Clone, Copy, Debug)]
pub struct IrConverter {
    pub from: Format,
    pub to: Format,
}

/// Everything a finished conversion knows about itself.
#[derive(Clone, Debug)]
pub struct ConversionReport {
    /// The produced files, in order.
    pub files: Vec<OutputFile>,
    /// Things the reader or writer could not carry, in plain language.
    pub warnings: Vec<String>,
    /// Structure of the parsed source, for the app's preview.
    pub summary: DocumentSummary,
    /// Format of the input that was parsed.
    pub from: Format,
    /// Format of the produced bytes.
    pub to: Format,
}

impl ConversionReport {
    /// The bytes of the first file, or an empty slice for an empty report.
    pub fn bytes(&self) -> &[u8] {
        self.files
            .first()
            .map(|file| file.bytes.as_slice())
            .unwrap_or(&[])
    }

    /// Total size of every produced file.
    pub fn total_bytes(&self) -> usize {
        self.files.iter().map(|file| file.bytes.len()).sum()
    }
}

impl Converter for IrConverter {
    fn source(&self) -> Format {
        self.from
    }

    fn target(&self) -> Format {
        self.to
    }

    fn convert(
        &self,
        input: &[u8],
        opts: &Options,
    ) -> Result<Vec<OutputFile>, ConvertError> {
        self.run(input, opts).map(|report| report.files)
    }

    fn run(&self, input: &[u8], opts: &Options) -> Result<ConversionReport, ConvertError> {
        if !is_supported(self.from, self.to) {
            return Err(unsupported(self.from, self.to));
        }
        opts.check_size(input)?;

        let document = read_document(self.from, input, opts)?;
        if document.blocks.is_empty() {
            return Err(ConvertError::EmptyDocument);
        }
        let warnings = collect_warnings(self.to, &document);
        let summary = document.summary();
        let bytes = writer_for(self.to).write(&document, opts)?;
        let name = match opts.output_stem.as_deref() {
            Some(stem) if !stem.trim().is_empty() => {
                format!("{}.{}", stem.trim(), self.to.extension())
            }
            _ => format!("output.{}", self.to.extension()),
        };

        Ok(ConversionReport {
            files: vec![OutputFile { name, bytes }],
            warnings,
            summary,
            from: self.from,
            to: self.to,
        })
    }

    fn run_with_progress(
        &self,
        input: &[u8],
        opts: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ConversionReport, ConvertError> {
        let report = self.run(input, opts)?;
        progress(1, 1);
        Ok(report)
    }
}

/// Parse a file into the document model without converting it.
pub fn read_document(
    format: Format,
    input: &[u8],
    options: &Options,
) -> Result<Document, ConvertError> {
    options.check_size(input)?;
    reader_for(format).read(input, options)
}

/// Every registered converter, in [`Format::ALL`] order.
pub fn registered() -> &'static [Box<dyn Converter>] {
    static REGISTRY: OnceLock<Vec<Box<dyn Converter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut converters: Vec<Box<dyn Converter>> = Vec::with_capacity(128);
        for from in Format::ALL {
            for to in Format::ALL {
                if !is_supported(from, to) {
                    continue;
                }
                if from == Format::Pdf && matches!(to, Format::Png | Format::Jpg) {
                    converters.push(Box::new(PdfToImageConverter {
                        to,
                        renderer: default_renderer(),
                    }));
                } else if from.is_image() && to == Format::Pdf {
                    converters.push(Box::new(ImageToPdfConverter { from }));
                } else if from.is_image() && to.is_image() {
                    converters.push(Box::new(ImageConverter { from, to }));
                } else {
                    converters.push(Box::new(IrConverter { from, to }));
                }
            }
        }
        converters
    })
}

/// The registered converter for a pair, or the typed reason it does not exist.
pub fn converter(from: Format, to: Format) -> Result<&'static dyn Converter, ConvertError> {
    registered()
        .iter()
        .find(|converter| converter.source() == from && converter.target() == to)
        .map(|converter| converter.as_ref())
        .ok_or_else(|| unsupported(from, to))
}

/// Convert a file. The shortest path in the API, for pairs that produce one file.
pub fn convert(
    from: Format,
    to: Format,
    input: &[u8],
    opts: &Options,
) -> Result<Vec<u8>, ConvertError> {
    let mut files = converter(from, to)?.convert(input, opts)?;
    if files.len() != 1 {
        return Err(ConvertError::other(format!(
            "This pair produced {} files; use convert_files to keep them all.",
            files.len()
        )));
    }
    Ok(files.remove(0).bytes)
}

/// Convert and keep every produced file.
pub fn convert_files(
    from: Format,
    to: Format,
    input: &[u8],
    opts: &Options,
) -> Result<Vec<OutputFile>, ConvertError> {
    converter(from, to)?.convert(input, opts)
}

/// Convert and keep the warnings and source statistics.
pub fn convert_report(
    from: Format,
    to: Format,
    input: &[u8],
    opts: &Options,
) -> Result<ConversionReport, ConvertError> {
    converter(from, to)?.run(input, opts)
}

/// Convert with per-page progress, keeping warnings and statistics.
pub fn convert_report_with_progress(
    from: Format,
    to: Format,
    input: &[u8],
    opts: &Options,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<ConversionReport, ConvertError> {
    converter(from, to)?.run_with_progress(input, opts, progress)
}

pub fn reader_for(format: Format) -> &'static dyn Reader {
    match format {
        Format::Txt => &readers::txt::READER,
        Format::Markdown => &readers::markdown::READER,
        Format::Html => &readers::html::READER,
        Format::Docx => &readers::docx::READER,
        Format::Pdf => &readers::pdf::READER,
        Format::Rtf => &readers::rtf::READER,
        Format::Json => &readers::json::READER,
        Format::Csv => &readers::csv::READER,
        Format::Png => &readers::image::PNG_READER,
        Format::Jpg => &readers::image::JPG_READER,
        Format::Webp => &readers::image::WEBP_READER,
        Format::Bmp => &readers::image::BMP_READER,
        Format::Gif => &readers::image::GIF_READER,
    }
}

pub fn writer_for(format: Format) -> &'static dyn Writer {
    match format {
        Format::Txt => &readers::txt::WRITER,
        Format::Markdown => &readers::markdown::WRITER,
        Format::Html => &readers::html::WRITER,
        Format::Docx => &readers::docx::WRITER,
        Format::Pdf => &readers::pdf::WRITER,
        Format::Rtf => &readers::rtf::WRITER,
        Format::Json => &readers::json::WRITER,
        Format::Csv => &readers::csv::WRITER,
        Format::Png => &readers::image::PNG_WRITER,
        Format::Jpg => &readers::image::JPG_WRITER,
        Format::Webp => &readers::image::WEBP_WRITER,
        Format::Bmp => &readers::image::BMP_WRITER,
        Format::Gif => &readers::image::GIF_WRITER,
    }
}

fn unsupported(from: Format, to: Format) -> ConvertError {
    let reason = pair_info(from, to)
        .notes
        .first()
        .copied()
        .unwrap_or("No converter is registered for this pair.");
    ConvertError::Unsupported { from, to, reason }
}

/// Post-conversion warnings: what the source had and the target could not keep.
fn collect_warnings(to: Format, document: &Document) -> Vec<String> {
    let mut warnings = document.notices.clone();

    let images = document.image_count();
    if images > 0 {
        match to {
            Format::Pdf | Format::Docx | Format::Rtf => warnings.push(format!(
                "{images} image{} left out: {to} output from Verso does not embed images yet.",
                if images == 1 { "" } else { "s" }
            )),
            Format::Txt | Format::Markdown | Format::Csv => {}
            _ => {}
        }
    }

    if to == Format::Pdf && document.has_non_winansi() {
        warnings.push(
            "Characters outside Windows-1252 were replaced with ? in the PDF.".to_string(),
        );
    }

    if to == Format::Csv {
        match document.table_count() {
            0 => warnings.push(
                "No table was found, so the text was exported as a single column.".to_string(),
            ),
            count if count > 1 => warnings.push(format!(
                "Only the first of {count} tables was exported; the rest were skipped."
            )),
            _ => {}
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_supported_pair() {
        let expected = Format::ALL
            .iter()
            .flat_map(|from| Format::ALL.iter().map(move |to| (*from, *to)))
            .filter(|(from, to)| is_supported(*from, *to))
            .count();
        assert_eq!(registered().len(), expected);
        assert_eq!(registered().len(), 95);
    }

    #[test]
    fn unsupported_pair_is_typed_and_explained() {
        let error = convert(Format::Pdf, Format::Csv, b"%PDF-1.7\n", &Options::default())
            .expect_err("unsupported");
        assert_eq!(error.code(), "unsupported-pair");
        assert!(error.to_string().contains("cannot be recovered"));
    }

    #[test]
    fn empty_input_is_refused() {
        let error = convert(Format::Txt, Format::Markdown, b"   \n\n", &Options::default())
            .expect_err("empty");
        assert_eq!(error.code(), "empty");
    }

    #[test]
    fn csv_without_a_table_explains_the_fallback() {
        let report = convert_report(
            Format::Txt,
            Format::Csv,
            b"just a sentence\n",
            &Options::default(),
        )
        .expect("converts");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("single column")));
        assert_eq!(report.summary.words, 3);
    }

    #[test]
    fn image_warning_names_the_target() {
        let input = b"<html><body><p><img alt=\"chart\" src=\"data:image/png;base64,aGVsbG8=\"></p></body></html>";
        let report = convert_report(Format::Html, Format::Pdf, input, &Options::default())
            .expect("converts");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("image") && warning.contains("PDF")));
    }

    #[test]
    fn every_registered_converter_matches_its_pair() {
        let mut seen = Vec::new();
        for converter in registered() {
            assert!(is_supported(converter.source(), converter.target()));
            let pair = (converter.source(), converter.target());
            assert!(!seen.contains(&pair), "duplicate converter for {pair:?}");
            seen.push(pair);
        }
    }
}
