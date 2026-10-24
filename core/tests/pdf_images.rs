//! Stage 2 tests: page fan-out, PDF embedding and typed render errors.

use verso_core::readers::pdf_image::UnavailableRenderer;
use verso_core::{
    convert, images_to_pdf, Converter, Format, ImageInput, Options, PageRenderer,
    PdfToImageConverter, RenderedPage,
};

struct FakeRenderer {
    pages: usize,
}

impl PageRenderer for FakeRenderer {
    fn render(
        &self,
        bytes: &[u8],
        _dpi: u16,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<RenderedPage>, verso_core::ConvertError> {
        assert!(bytes.starts_with(b"%PDF-"), "a real PDF reaches the renderer");
        progress(0, self.pages);
        let mut pages = Vec::new();
        for index in 0..self.pages {
            pages.push(RenderedPage {
                width: 20,
                height: 12,
                rgba: vec![200, 100, 40, 255].repeat(20 * 12),
            });
            progress(index + 1, self.pages);
        }
        Ok(pages)
    }
}

fn blank_pdf() -> Vec<u8> {
    b"%PDF-1.7\n% fake fixture for the renderer\n".to_vec()
}

fn png(width: u32, height: u32) -> Vec<u8> {
    use image::{ExtendedColorType, ImageEncoder};
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&[20, 140, 90, 255]);
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .expect("fixture");
    bytes
}

#[test]
fn multi_page_pdf_writes_one_named_file_per_page() {
    static RENDERER: FakeRenderer = FakeRenderer { pages: 3 };
    let converter = PdfToImageConverter {
        to: Format::Png,
        renderer: &RENDERER,
    };
    let options = Options {
        output_stem: Some("report".to_string()),
        ..Options::default()
    };

    let files = converter
        .convert(&blank_pdf(), &options)
        .expect("three pages render");

    let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "report_page-001.png",
            "report_page-002.png",
            "report_page-003.png"
        ]
    );
    for file in &files {
        assert!(file.bytes.starts_with(b"\x89PNG"));
        let info = verso_core::image_info(&file.bytes).expect("page is a PNG");
        assert_eq!((info.width, info.height), (20, 12));
    }
}

#[test]
fn jpg_pages_use_the_jpg_extension() {
    static RENDERER: FakeRenderer = FakeRenderer { pages: 2 };
    let converter = PdfToImageConverter {
        to: Format::Jpg,
        renderer: &RENDERER,
    };
    let options = Options {
        output_stem: Some("scan".to_string()),
        ..Options::default()
    };
    let files = converter.convert(&blank_pdf(), &options).expect("renders");
    assert_eq!(files[0].name, "scan_page-001.jpg");
    assert_eq!(files[1].name, "scan_page-002.jpg");
}

#[test]
fn missing_pdfium_is_a_typed_error_not_a_panic() {
    static RENDERER: UnavailableRenderer = UnavailableRenderer {
        detail: "PDFium is not installed for this test.",
    };
    let converter = PdfToImageConverter {
        to: Format::Png,
        renderer: &RENDERER,
    };
    let error = converter
        .convert(&blank_pdf(), &Options::default())
        .expect_err("must fail cleanly");
    assert_eq!(error.code(), "pdf-render-unavailable");
    assert!(error.to_string().contains("PDFium is not installed"));
}

fn embedded_image_size(pdf: &[u8]) -> Option<(i64, i64)> {
    // Our writer emits image dictionaries uncompressed, e.g.
    // `/Subtype/Image/Width 80/Height 50/...`.
    let text = String::from_utf8_lossy(pdf);
    let start = text.find("/Subtype/Image")?;
    let tail = &text[start..];
    let width = number_after(tail, "/Width ")?;
    let height = number_after(tail, "/Height ")?;
    Some((width, height))
}

fn number_after(text: &str, key: &str) -> Option<i64> {
    let start = text.find(key)? + key.len();
    let digits: String = text[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn media_box_size(pdf: &[u8]) -> Option<(f32, f32)> {
    let document = lopdf::Document::load_mem(pdf).ok()?;
    let pages = document.get_pages();
    let page_id = *pages.values().next()?;
    let dictionary = document.get_dictionary(page_id).ok()?;
    let media_box = dictionary.get(b"MediaBox").ok()?.as_array().ok()?;
    let values: Vec<f32> = media_box
        .iter()
        .filter_map(|value| {
            value
                .as_float()
                .ok()
                .or_else(|| value.as_i64().ok().map(|number| number as f32))
        })
        .collect();
    if values.len() < 4 {
        return None;
    }
    Some((values[2] - values[0], values[3] - values[1]))
}

#[test]
fn image_pdf_round_trip_keeps_dimensions() {
    let png = png(80, 50);
    let pdf = convert(Format::Png, Format::Pdf, &png, &Options::default()).expect("image → PDF");

    let (width, height) = embedded_image_size(&pdf).expect("the PDF carries an image");
    assert!((width - 80).abs() <= 1, "width {width}");
    assert!((height - 50).abs() <= 1, "height {height}");

    // The page itself is the image size at 96 DPI, so a rasterizer at 96 DPI
    // returns the original dimensions within a point.
    let (page_w, page_h) = media_box_size(&pdf).expect("MediaBox");
    let px_w = page_w * 96.0 / 72.0;
    let px_h = page_h * 96.0 / 72.0;
    assert!((px_w - 80.0).abs() <= 1.0, "page width in pixels {px_w}");
    assert!((px_h - 50.0).abs() <= 1.0, "page height in pixels {px_h}");
}

#[test]
fn jpeg_bytes_are_embedded_without_re_encoding() {
    use image::{ExtendedColorType, ImageEncoder};
    let mut rgb = Vec::new();
    for _ in 0..64 * 64 {
        rgb.extend_from_slice(&[210, 90, 40]);
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 70)
        .write_image(&rgb, 64, 64, ExtendedColorType::Rgb8)
        .expect("fixture");

    let pdf = convert(Format::Jpg, Format::Pdf, &jpeg, &Options::default()).expect("jpg → PDF");
    let (width, height) = embedded_image_size(&pdf).expect("image dictionary");
    assert_eq!((width, height), (64, 64));

    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("DCTDecode"), "expected a DCTDecode image");
    assert!(
        pdf.windows(jpeg.len()).any(|window| window == jpeg.as_slice()),
        "the original JPEG bytes should be present verbatim"
    );
}

#[test]
fn combine_two_images_makes_two_pages() {
    let first = ImageInput {
        format: Format::Png,
        bytes: png(40, 30),
    };
    let second = ImageInput {
        format: Format::Png,
        bytes: png(20, 60),
    };
    let (pdf, _) = images_to_pdf(&[first, second], &Options::default()).expect("combined PDF");
    let document = lopdf::Document::load_mem(&pdf).expect("parse");
    assert_eq!(document.get_pages().len(), 2);
}

#[test]
fn image_only_pdf_reads_as_a_scan_not_damage() {
    let pdf = convert(Format::Png, Format::Pdf, &png(20, 20), &Options::default())
        .expect("image → PDF");
    let error = verso_core::read_document(Format::Pdf, &pdf, &Options::default())
        .expect_err("an image-only PDF has no text layer");
    assert_eq!(error.code(), "scanned-pdf", "{error}");
}

#[test]
fn corrupt_image_to_pdf_is_typed() {
    let error = convert(Format::Png, Format::Pdf, b"not a png", &Options::default())
        .expect_err("must fail");
    assert_eq!(error.code(), "parse");
}

#[test]
fn page_size_option_changes_the_media_box() {
    let options = Options {
        image: verso_core::ImageOptions {
            page_size: verso_core::ImagePageSize::A4,
            ..verso_core::ImageOptions::default()
        },
        ..Options::default()
    };
    let pdf = convert(Format::Png, Format::Pdf, &png(30, 30), &options).expect("a4 PDF");
    let document = lopdf::Document::load_mem(&pdf).expect("parse");
    let pages = document.get_pages();
    let page_id = *pages.values().next().expect("one page");
    let media_box = document
        .get_dictionary(page_id)
        .ok()
        .and_then(|dict| dict.get(b"MediaBox").ok().cloned())
        .expect("MediaBox");
    let numbers: Vec<f32> = media_box
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_float().ok().or_else(|| value.as_i64().ok().map(|v| v as f32)))
                .collect()
        })
        .unwrap_or_default();
    assert!(numbers.len() >= 4, "MediaBox: {numbers:?}");
    let width = numbers[2] - numbers[0];
    let height = numbers[3] - numbers[1];
    assert!((width - 595.28).abs() < 2.0, "A4 width {width}");
    assert!((height - 841.89).abs() < 2.0, "A4 height {height}");
}
