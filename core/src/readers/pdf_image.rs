//! PDF → image: rasterize each page through PDFium.
//!
//! The renderer is behind a small trait so the page fan-out and naming can be
//! tested without a PDFium binary. The real backend loads PDFium at runtime and
//! reports [`ConvertError::PdfRenderUnavailable`] when it is missing.

#[cfg(feature = "pdf-render")]
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::document::DocumentSummary;
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::registry::{ConversionReport, Converter, OutputFile};

/// One rendered page, as straight RGBA bytes.
#[derive(Clone, Debug)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A PDF page rasterizer.
pub trait PageRenderer: Send + Sync {
    fn render(
        &self,
        bytes: &[u8],
        dpi: u16,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<RenderedPage>, ConvertError>;
}

/// Registered converter for PDF → PNG/JPG.
#[derive(Clone, Copy)]
pub struct PdfToImageConverter {
    pub to: Format,
    pub renderer: &'static dyn PageRenderer,
}

impl Converter for PdfToImageConverter {
    fn source(&self) -> Format {
        Format::Pdf
    }

    fn target(&self) -> Format {
        self.to
    }

    fn convert(
        &self,
        input: &[u8],
        options: &Options,
    ) -> Result<Vec<OutputFile>, ConvertError> {
        let mut progress = |_, _| {};
        self.render(input, options, &mut progress)
    }

    fn convert_with_progress(
        &self,
        input: &[u8],
        options: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<OutputFile>, ConvertError> {
        self.render(input, options, progress)
    }

    fn run_with_progress(
        &self,
        input: &[u8],
        options: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ConversionReport, ConvertError> {
        let dpi = options.image.dpi.clamp(72, 300);
        let files = self.render(input, options, progress)?;
        let pages = files.len();
        Ok(ConversionReport {
            files,
            warnings: vec![format!(
                "Each page was rasterized at {dpi} DPI; text is no longer selectable."
            )],
            summary: DocumentSummary {
                blocks: pages,
                images: pages,
                ..DocumentSummary::default()
            },
            from: Format::Pdf,
            to: self.to,
        })
    }
}

impl PdfToImageConverter {
    fn render(
        &self,
        input: &[u8],
        options: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<OutputFile>, ConvertError> {
        options.check_size(input)?;
        let dpi = options.image.dpi.clamp(72, 300);
        let pages = self.renderer.render(input, dpi, progress)?;
        if pages.is_empty() {
            return Err(ConvertError::parse(Format::Pdf, "the PDF has no pages"));
        }

        let stem = options
            .output_stem
            .as_deref()
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .unwrap_or("page");
        let mut files = Vec::with_capacity(pages.len());
        for (index, page) in pages.into_iter().enumerate() {
            let rgba = image::RgbaImage::from_raw(page.width, page.height, page.rgba)
                .ok_or_else(|| {
                    ConvertError::PdfRender {
                        detail: "the rendered bitmap has an unexpected size".to_string(),
                    }
                })?;
            let bytes =
                crate::readers::image::encode_rgba(rgba, self.to, options.image.quality)?;
            files.push(OutputFile {
                name: format!(
                    "{stem}_page-{:03}.{}",
                    index + 1,
                    self.to.extension()
                ),
                bytes,
            });
        }
        Ok(files)
    }
}

/// The renderer used by the registry.
pub fn default_renderer() -> &'static dyn PageRenderer {
    #[cfg(feature = "pdf-render")]
    {
        &PDFIUM_RENDERER
    }
    #[cfg(not(feature = "pdf-render"))]
    {
        &UNAVAILABLE_RENDERER
    }
}

pub struct UnavailableRenderer {
    pub detail: &'static str,
}

impl PageRenderer for UnavailableRenderer {
    fn render(
        &self,
        _bytes: &[u8],
        _dpi: u16,
        _progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<RenderedPage>, ConvertError> {
        Err(ConvertError::PdfRenderUnavailable {
            detail: self.detail.to_string(),
        })
    }
}

#[cfg(not(feature = "pdf-render"))]
static UNAVAILABLE_RENDERER: UnavailableRenderer = UnavailableRenderer {
    detail: "This build was compiled without the pdf-render feature.",
};

#[cfg(not(feature = "pdf-render"))]
pub fn missing_pdfium_message() -> &'static str {
    "This build was compiled without the pdf-render feature."
}

// ---------------------------------------------------------------------------
// PDFium backend
// ---------------------------------------------------------------------------

#[cfg(feature = "pdf-render")]
pub struct PdfiumRenderer;

#[cfg(feature = "pdf-render")]
impl PageRenderer for PdfiumRenderer {
    fn render(
        &self,
        bytes: &[u8],
        dpi: u16,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<RenderedPage>, ConvertError> {
        match catch_unwind(AssertUnwindSafe(|| render_with_pdfium(bytes, dpi, progress))) {
            Ok(result) => result,
            Err(_) => Err(ConvertError::PdfRenderUnavailable {
                detail: "PDFium stopped unexpectedly while rendering. Install a matching PDFium build from https://github.com/bblanchon/pdfium-binaries/releases".to_string(),
            }),
        }
    }
}

#[cfg(feature = "pdf-render")]
static PDFIUM_RENDERER: PdfiumRenderer = PdfiumRenderer;

#[cfg(feature = "pdf-render")]
fn render_with_pdfium(
    bytes: &[u8],
    dpi: u16,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<RenderedPage>, ConvertError> {
    use pdfium_render::prelude::PdfRenderConfig;

    let pdfium = bind_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|_| ConvertError::PdfUnreadable)?;

    let total = usize::from(document.pages().len());
    if total == 0 {
        return Err(ConvertError::parse(Format::Pdf, "the PDF has no pages"));
    }
    progress(0, total);

    let config = PdfRenderConfig::new()
        .scale_page_by_factor(f32::from(dpi) / 72.0)
        .render_form_data(true)
        .render_annotations(true);

    let mut pages = Vec::with_capacity(total);
    for (index, page) in document.pages().iter().enumerate() {
        let bitmap = page
            .render_with_config(&config)
            .map_err(|error| ConvertError::PdfRender {
                detail: format!("page {}: {error}", index + 1),
            })?;
        let width = bitmap.width() as u32;
        let height = bitmap.height() as u32;
        let rgba = bitmap.as_rgba_bytes();
        pages.push(RenderedPage {
            width,
            height,
            rgba,
        });
        progress(index + 1, total);
    }
    Ok(pages)
}

#[cfg(feature = "pdf-render")]
fn bind_pdfium() -> Result<pdfium_render::prelude::Pdfium, ConvertError> {
    use pdfium_render::prelude::Pdfium;
    use std::path::PathBuf;

    let missing = || ConvertError::PdfRenderUnavailable {
        detail: "PDF rendering needs the PDFium library. Download it from https://github.com/bblanchon/pdfium-binaries/releases and put pdfium.dll next to verso.exe, on PATH, or set VERSO_PDFIUM_PATH.".to_string(),
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(value) = std::env::var("VERSO_PDFIUM_PATH") {
        let path = PathBuf::from(value);
        if path.is_dir() {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(&path));
        } else {
            candidates.push(path);
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(directory));
        }
    }
    candidates.push(Pdfium::pdfium_platform_library_name_at_path(""));

    for candidate in candidates {
        if let Ok(bindings) = Pdfium::bind_to_library(&candidate) {
            return Ok(Pdfium::new(bindings));
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|_| missing())
}
