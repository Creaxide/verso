//! Image → PDF through `printpdf`.
//!
//! One page per image. JPEG bytes are embedded directly when no resize is
//! requested, so the original compression survives. Other formats are decoded
//! and placed as pixel data (with their alpha where the format has it).

use std::io::Cursor;

use printpdf::{
    ColorBits, ColorSpace, Image, ImageFilter, ImageTransform, ImageXObject, Mm, PdfDocument, Px,
};

use crate::document::DocumentSummary;
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::{ImagePageSize, Options};
use crate::registry::{ConversionReport, Converter, OutputFile};

/// One image handed to [`images_to_pdf`].
#[derive(Clone, Debug)]
pub struct ImageInput {
    pub format: Format,
    pub bytes: Vec<u8>,
}

/// A page's size and where the image sits on it, in millimetres.
#[derive(Clone, Copy, Debug)]
struct Placement {
    page_w: f32,
    page_h: f32,
    draw_w: f32,
    draw_h: f32,
    x: f32,
    y: f32,
}

enum Prepared {
    Jpeg {
        bytes: Vec<u8>,
        width: u32,
        height: u32,
        grayscale: bool,
    },
    Pixels {
        image: image::DynamicImage,
        width: u32,
        height: u32,
    },
}

impl Prepared {
    fn load(input: &ImageInput, options: &Options) -> Result<Self, ConvertError> {
        options.check_size(&input.bytes)?;

        // JPEG without a resize is embedded as-is.
        if input.format == Format::Jpg && options.image.max_dimension.is_none() {
            if let Some((width, height, grayscale)) = jpeg_metadata(&input.bytes) {
                return Ok(Prepared::Jpeg {
                    bytes: input.bytes.clone(),
                    width,
                    height,
                    grayscale,
                });
            }
        }

        let (image, _) = crate::readers::image::decode_image(input.format, &input.bytes)?;
        let image = match options.image.max_dimension {
            Some(max) => {
                let longest = image.width().max(image.height());
                if longest > max {
                    let scale = max as f32 / longest as f32;
                    let width = ((image.width() as f32 * scale).round() as u32).max(1);
                    let height = ((image.height() as f32 * scale).round() as u32).max(1);
                    image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
                } else {
                    image
                }
            }
            None => image,
        };
        let width = image.width();
        let height = image.height();
        Ok(Prepared::Pixels {
            image,
            width,
            height,
        })
    }

    fn width(&self) -> u32 {
        match self {
            Prepared::Jpeg { width, .. } | Prepared::Pixels { width, .. } => *width,
        }
    }

    fn height(&self) -> u32 {
        match self {
            Prepared::Jpeg { height, .. } | Prepared::Pixels { height, .. } => *height,
        }
    }
}

/// Read JPEG dimensions and colour layout without decoding pixels.
fn jpeg_metadata(bytes: &[u8]) -> Option<(u32, u32, bool)> {
    use image::ImageDecoder;
    let decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    let grayscale = matches!(
        decoder.color_type(),
        image::ColorType::L8 | image::ColorType::L16
    );
    Some((width, height, grayscale))
}

/// Convert a set of images into one PDF, one image per page.
pub fn images_to_pdf(
    images: &[ImageInput],
    options: &Options,
) -> Result<(Vec<u8>, Vec<String>), ConvertError> {
    if images.is_empty() {
        return Err(ConvertError::EmptyDocument);
    }
    options.check_size(&images[0].bytes)?;

    let mut prepared = Vec::with_capacity(images.len());
    for image in images {
        prepared.push(Prepared::load(image, options)?);
    }

    let placements: Vec<Placement> = prepared
        .iter()
        .map(|image| placement(image.width(), image.height(), options))
        .collect();

    let first = placements[0];
    let (doc, first_page, first_layer) =
        PdfDocument::new("Verso", Mm(first.page_w), Mm(first.page_h), "Layer 1");

    let mut warnings = Vec::new();
    let mut embedded_jpeg = 0usize;
    let dpi = 96.0f32;

    for (index, (ready, place)) in prepared.into_iter().zip(placements).enumerate() {
        let (page, layer) = if index == 0 {
            (first_page, first_layer)
        } else {
            doc.add_page(Mm(place.page_w), Mm(place.page_h), "Layer 1")
        };
        let layer = doc.get_page(page).get_layer(layer);
        let transform = ImageTransform {
            translate_x: Some(Mm(place.x)),
            translate_y: Some(Mm(place.y)),
            scale_x: Some(place.draw_w / px_to_mm(ready.width(), dpi)),
            scale_y: Some(place.draw_h / px_to_mm(ready.height(), dpi)),
            dpi: Some(dpi),
            ..ImageTransform::default()
        };

        match ready {
            Prepared::Jpeg {
                bytes,
                width,
                height,
                grayscale,
            } => {
                embedded_jpeg += 1;
                let xobject = ImageXObject {
                    width: Px(width as usize),
                    height: Px(height as usize),
                    color_space: if grayscale {
                        ColorSpace::Greyscale
                    } else {
                        ColorSpace::Rgb
                    },
                    bits_per_component: ColorBits::Bit8,
                    interpolate: true,
                    image_data: bytes,
                    image_filter: Some(ImageFilter::DCT),
                    smask: None,
                    clipping_bbox: None,
                };
                Image::from(xobject).add_to_layer(layer, transform);
            }
            Prepared::Pixels { image, .. } => {
                // printpdf 0.7 still consumes image 0.24; hand it a buffer.
                let rgba = image.to_rgba8();
                let (width, height) = rgba.dimensions();
                let buffer = image24::RgbaImage::from_raw(width, height, rgba.into_raw())
                    .ok_or_else(|| ConvertError::PdfWrite {
                        detail: "the image buffer could not be prepared for the PDF".to_string(),
                    })?;
                let print_image = image24::DynamicImage::ImageRgba8(buffer);
                Image::from_dynamic_image(&print_image).add_to_layer(layer, transform);
            }
        }
    }

    if embedded_jpeg > 0 {
        warnings.push(format!(
            "{embedded_jpeg} JPEG image{} embedded without re-encoding.",
            if embedded_jpeg == 1 { "" } else { "s" }
        ));
    }

    let bytes = doc
        .save_to_bytes()
        .map_err(|error| ConvertError::PdfWrite {
            detail: error.to_string(),
        })?;
    Ok((bytes, warnings))
}

fn placement(width: u32, height: u32, options: &Options) -> Placement {
    let margin_mm = options.pdf.margin_pt * 25.4 / 72.0;
    match options.image.page_size {
        ImagePageSize::MatchImage => {
            let page_w = px_to_mm(width, 96.0).max(1.0);
            let page_h = px_to_mm(height, 96.0).max(1.0);
            Placement {
                page_w,
                page_h,
                draw_w: page_w,
                draw_h: page_h,
                x: 0.0,
                y: 0.0,
            }
        }
        size => {
            let (pw_pt, ph_pt) = size.points().unwrap_or((595.28, 841.89));
            let page_w = pw_pt * 25.4 / 72.0;
            let page_h = ph_pt * 25.4 / 72.0;
            let content_w = (page_w - margin_mm * 2.0).max(1.0);
            let content_h = (page_h - margin_mm * 2.0).max(1.0);
            let image_w = px_to_mm(width, 96.0).max(1.0);
            let image_h = px_to_mm(height, 96.0).max(1.0);
            let scale = (content_w / image_w).min(content_h / image_h);
            let draw_w = image_w * scale;
            let draw_h = image_h * scale;
            Placement {
                page_w,
                page_h,
                draw_w,
                draw_h,
                x: (page_w - draw_w) / 2.0,
                y: (page_h - draw_h) / 2.0,
            }
        }
    }
}

fn px_to_mm(pixels: u32, dpi: f32) -> f32 {
    pixels as f32 * 25.4 / dpi
}

/// Registered converter for a single image → PDF.
#[derive(Clone, Copy, Debug)]
pub struct ImageToPdfConverter {
    pub from: Format,
}

impl Converter for ImageToPdfConverter {
    fn source(&self) -> Format {
        self.from
    }

    fn target(&self) -> Format {
        Format::Pdf
    }

    fn convert(
        &self,
        input: &[u8],
        options: &Options,
    ) -> Result<Vec<OutputFile>, ConvertError> {
        let (bytes, _) = images_to_pdf(
            &[ImageInput {
                format: self.from,
                bytes: input.to_vec(),
            }],
            options,
        )?;
        let stem = options
            .output_stem
            .as_deref()
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .unwrap_or("images");
        Ok(vec![OutputFile {
            name: format!("{stem}.pdf"),
            bytes,
        }])
    }

    fn run(
        &self,
        input: &[u8],
        options: &Options,
    ) -> Result<ConversionReport, ConvertError> {
        let (bytes, warnings) = images_to_pdf(
            &[ImageInput {
                format: self.from,
                bytes: input.to_vec(),
            }],
            options,
        )?;
        let stem = options
            .output_stem
            .as_deref()
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .unwrap_or("images");
        Ok(ConversionReport {
            files: vec![OutputFile {
                name: format!("{stem}.pdf"),
                bytes,
            }],
            warnings,
            summary: DocumentSummary {
                blocks: 1,
                images: 1,
                ..DocumentSummary::default()
            },
            from: self.from,
            to: Format::Pdf,
        })
    }

    fn run_with_progress(
        &self,
        input: &[u8],
        options: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ConversionReport, ConvertError> {
        let report = self.run(input, options)?;
        progress(1, 1);
        Ok(report)
    }
}
