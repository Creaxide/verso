//! Image readers, writers and the image ↔ image converter.
//!
//! All five raster formats go through one [`ImageConverter`]: decode into an
//! RGBA/RGB buffer, optionally resize, flatten alpha when the target has none,
//! and encode. GIF is read one frame at a time; only the first frame is used.

use std::io::Cursor;

use image::codecs::gif::{GifDecoder, GifEncoder};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{
    AnimationDecoder, DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat as ImgFormat,
};

use crate::document::{Block, Document, DocumentSummary, ImageRef};
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::registry::OutputFile;
use crate::traits::{Reader, Writer};

/// Images larger than this on either side, or with more pixels than
/// [`LARGE_IMAGE_PIXELS`], are flagged to the app before conversion.
pub const LARGE_IMAGE_SIDE: u32 = 10_000;
pub const LARGE_IMAGE_PIXELS: u64 = 100_000_000;

/// Header-only information about an image, used for the preview and the
/// large-image confirmation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

impl ImageInfo {
    pub fn is_large(self) -> bool {
        self.width > LARGE_IMAGE_SIDE
            || self.height > LARGE_IMAGE_SIDE
            || u64::from(self.width) * u64::from(self.height) > LARGE_IMAGE_PIXELS
    }
}

/// Read the pixel dimensions without decoding the whole image.
pub fn image_info(bytes: &[u8]) -> Result<ImageInfo, ConvertError> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ConvertError::parse(Format::Png, error))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| ConvertError::parse(Format::Png, error))?;
    Ok(ImageInfo { width, height })
}

fn image_format(format: Format) -> Option<ImgFormat> {
    match format {
        Format::Png => Some(ImgFormat::Png),
        Format::Jpg => Some(ImgFormat::Jpeg),
        Format::Webp => Some(ImgFormat::WebP),
        Format::Bmp => Some(ImgFormat::Bmp),
        Format::Gif => Some(ImgFormat::Gif),
        _ => None,
    }
}

/// Decode bytes into an image, reporting whether a GIF had more than one frame.
fn decode(format: Format, bytes: &[u8]) -> Result<(DynamicImage, bool), ConvertError> {    if format == Format::Gif {
        return decode_gif(bytes);
    }
    let img_format = image_format(format)
        .ok_or_else(|| ConvertError::parse(format, "not an image format"))?;
    let image = image::load_from_memory_with_format(bytes, img_format)
        .map_err(|error| ConvertError::parse(format, describe_decode_error(error)))?;
    Ok((image, false))
}

fn decode_gif(bytes: &[u8]) -> Result<(DynamicImage, bool), ConvertError> {
    let decoder = GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| ConvertError::parse(Format::Gif, describe_decode_error(error)))?;
    let mut frames = decoder.into_frames();
    let first = frames
        .next()
        .ok_or_else(|| ConvertError::parse(Format::Gif, "the file contains no frames"))?
        .map_err(|error| ConvertError::parse(Format::Gif, describe_decode_error(error)))?;
    let animated = frames.next().is_some();
    Ok((DynamicImage::ImageRgba8(first.into_buffer()), animated))
}

fn describe_decode_error(error: image::ImageError) -> String {
    match error {
        image::ImageError::Unsupported(_) => {
            "the file uses an image feature this build cannot read".to_string()
        }
        image::ImageError::Decoding(_) => {
            "the file is damaged or is not the image its extension says".to_string()
        }
        other => other.to_string(),
    }
}

/// Downscale so neither side exceeds `max`, keeping the aspect ratio. Images
/// already inside the limit are returned untouched.
fn fit_within(image: DynamicImage, max: u32) -> (DynamicImage, bool) {
    let longest = image.width().max(image.height());
    if longest <= max || max == 0 {
        return (image, false);
    }
    let scale = max as f32 / longest as f32;
    let width = ((image.width() as f32 * scale).round() as u32).max(1);
    let height = ((image.height() as f32 * scale).round() as u32).max(1);
    (image.resize_exact(width, height, FilterType::Lanczos3), true)
}

/// Flatten any alpha onto white, which is what JPG and BMP require.
fn flatten_on_white(image: &DynamicImage) -> image::RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = u32::from(pixel[3]);
        let blend = |channel: u8| -> u8 {
            ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8
        };
        rgb.put_pixel(
            x,
            y,
            image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    rgb
}

fn encode(image: DynamicImage, format: Format, options: &Options) -> Result<Vec<u8>, ConvertError> {
    encode_with_quality(image, format, options.image.quality)
}

pub(crate) fn encode_rgba(
    rgba: image::RgbaImage,
    format: Format,
    quality: u8,
) -> Result<Vec<u8>, ConvertError> {
    encode_with_quality(DynamicImage::ImageRgba8(rgba), format, quality)
}

fn encode_with_quality(
    image: DynamicImage,
    format: Format,
    quality: u8,
) -> Result<Vec<u8>, ConvertError> {
    let quality = quality.clamp(1, 100);
    let mut bytes = Vec::new();

    match format {
        Format::Png => {
            let rgba = image.to_rgba8();
            PngEncoder::new(&mut bytes)
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    ExtendedColorType::Rgba8,
                )
                .map_err(|error| ConvertError::encode(format, error))?;
        }
        Format::Jpg => {
            let rgb = flatten_on_white(&image);
            JpegEncoder::new_with_quality(&mut bytes, quality)
                .encode_image(&rgb)
                .map_err(|error| ConvertError::encode(format, error))?;
        }
        Format::Bmp => {
            let rgb = flatten_on_white(&image);
            image::codecs::bmp::BmpEncoder::new(&mut bytes)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .map_err(|error| ConvertError::encode(format, error))?;
        }
        Format::Webp => {
            let rgba = image.to_rgba8();
            let encoded = webp::Encoder::from_rgba(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
            )
            .encode(f32::from(quality));
            bytes.extend_from_slice(&encoded);
        }
        Format::Gif => {
            let rgba = image.to_rgba8();
            GifEncoder::new(&mut bytes)
                .encode(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    ExtendedColorType::Rgba8,
                )
                .map_err(|error| ConvertError::encode(format, error))?;
        }
        _ => return Err(ConvertError::encode(format, "not an image format")),
    }

    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Converter
// ---------------------------------------------------------------------------

/// Every image → image pair. One type, parameterised by the pair, exactly like
/// [`crate::registry::IrConverter`] does for documents.
#[derive(Clone, Copy, Debug)]
pub struct ImageConverter {
    pub from: Format,
    pub to: Format,
}

impl crate::registry::Converter for ImageConverter {
    fn source(&self) -> Format {
        self.from
    }

    fn target(&self) -> Format {
        self.to
    }

    fn convert(
        &self,
        input: &[u8],
        options: &Options,
    ) -> Result<Vec<OutputFile>, ConvertError> {
        self.run(input, options).map(|report| report.files)
    }

    fn run(
        &self,
        input: &[u8],
        options: &Options,
    ) -> Result<crate::registry::ConversionReport, ConvertError> {
        options.check_size(input)?;
        let (image, animated) = decode(self.from, input)?;
        let (image, resized) = match options.image.max_dimension {
            Some(max) => fit_within(image, max),
            None => (image, false),
        };

        let mut warnings = Vec::new();
        if animated {
            warnings.push(
                "Only the first frame of this animated GIF was used; the animation is not preserved."
                    .to_string(),
            );
        }
        if self.from.has_alpha() && !self.to.has_alpha() {
            warnings.push(format!(
                "Transparency was flattened onto white because {} has no alpha channel.",
                self.to.code()
            ));
        }
        if resized {
            warnings.push(format!(
                "The image was scaled down to fit within {} px.",
                options.image.max_dimension.unwrap_or_default()
            ));
        }
        if self.to == Format::Gif {
            warnings.push("GIF output is limited to a 256-colour palette.".to_string());
        }
        if self.from == self.to && matches!(self.to, Format::Jpg | Format::Webp) {
            warnings.push(
                "The image was re-encoded, which adds another generation of compression."
                    .to_string(),
            );
        }

        let bytes = encode(image, self.to, options)?;
        let name = match options.output_stem.as_deref() {
            Some(stem) if !stem.trim().is_empty() => {
                format!("{}.{}", stem.trim(), self.to.extension())
            }
            _ => format!("image.{}", self.to.extension()),
        };

        Ok(crate::registry::ConversionReport {
            files: vec![OutputFile { name, bytes }],
            warnings,
            summary: DocumentSummary {
                blocks: 1,
                images: 1,
                ..DocumentSummary::default()
            },
            from: self.from,
            to: self.to,
        })
    }

    fn run_with_progress(
        &self,
        input: &[u8],
        options: &Options,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<crate::registry::ConversionReport, ConvertError> {
        let report = self.run(input, options)?;
        progress(1, 1);
        Ok(report)
    }
}

/// Shared helper for Stage 2: decode and encode a single image buffer.
pub(crate) fn transcode(
    from: Format,
    to: Format,
    input: &[u8],
    options: &Options,
) -> Result<Vec<u8>, ConvertError> {
    let (image, _) = decode(from, input)?;
    let (image, _) = match options.image.max_dimension {
        Some(max) => fit_within(image, max),
        None => (image, false),
    };
    encode(image, to, options)
}

// ---------------------------------------------------------------------------
// Reader / writer
// ---------------------------------------------------------------------------

/// Reads an image into a one-block document, carrying the original bytes.
pub struct ImageReader {
    pub format: Format,
}

impl Reader for ImageReader {
    fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError> {
        options.check_size(input)?;
        let (image, animated) = decode(self.format, input)?;
        let alt = format!(
            "{} × {} {}",
            image.width(),
            image.height(),
            self.format.code()
        );
        let reference = ImageRef {
            alt,
            mime: self.format.mime().to_string(),
            data: Some(input.to_vec()),
            source: None,
        };
        let mut document = Document::new(vec![Block::Image(reference)]);
        if animated {
            document.notices.push(
                "Only the first frame of this animated GIF is used.".to_string(),
            );
        }
        Ok(document)
    }
}

/// Writes the first image carried by a document, used when a document is
/// turned into an image target.
pub struct ImageWriter {
    pub format: Format,
}

impl Writer for ImageWriter {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError> {
        let data = document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Image(image) => image.data.as_deref(),
                _ => None,
            })
            .ok_or_else(|| {
                ConvertError::encode(self.format, "the document carries no image data")
            })?;
        let source = Format::sniff(data);
        if !source.is_image() {
            return Err(ConvertError::encode(
                self.format,
                "the attached image is not a readable raster format",
            ));
        }
        transcode(source, self.format, data, options)
    }
}

pub static PNG_READER: ImageReader = ImageReader { format: Format::Png };
pub static JPG_READER: ImageReader = ImageReader { format: Format::Jpg };
pub static WEBP_READER: ImageReader = ImageReader { format: Format::Webp };
pub static BMP_READER: ImageReader = ImageReader { format: Format::Bmp };
pub static GIF_READER: ImageReader = ImageReader { format: Format::Gif };

pub static PNG_WRITER: ImageWriter = ImageWriter { format: Format::Png };
pub static JPG_WRITER: ImageWriter = ImageWriter { format: Format::Jpg };
pub static WEBP_WRITER: ImageWriter = ImageWriter { format: Format::Webp };
pub static BMP_WRITER: ImageWriter = ImageWriter { format: Format::Bmp };
pub static GIF_WRITER: ImageWriter = ImageWriter { format: Format::Gif };

/// Decode for other modules (image → PDF).
pub(crate) fn decode_image(
    format: Format,
    bytes: &[u8],
) -> Result<(DynamicImage, bool), ConvertError> {
    decode(format, bytes)
}
