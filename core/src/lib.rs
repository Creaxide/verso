//! Verso's conversion core.
//!
//! The core knows nothing about windows, files pickers or themes. It exposes:
//!
//! - [`Format`], the thirteen formats Verso understands;
//! - [`Document`], the intermediate model every reader produces;
//! - [`Converter`], the byte-in/byte-out contract, one per supported pair;
//! - [`convert`] and [`convert_report`], the convenient entry points;
//! - [`paths`], which guarantees outputs never replace anything.
//!
//! A typical call looks like this:
//!
//! ```no_run
//! use verso_core::{convert, Format, Options};
//!
//! let bytes = std::fs::read("notes.txt")?;
//! let markdown = convert(Format::Txt, Format::Markdown, &bytes, &Options::default())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

pub mod document;
pub mod error;
pub mod format;
pub mod matrix;
pub mod options;
pub mod paths;
pub mod readers;
pub mod registry;
pub mod traits;
pub mod util;

pub use document::{Align, Block, Document, DocumentSummary, ImageRef, Inline, ListItem, Table};
pub use error::ConvertError;
pub use format::Format;
pub use matrix::{conversion_matrix, is_supported, pair_info, Pair, Support};
pub use options::{ImageOptions, ImagePageSize, Options, PageSize, PdfOptions};
pub use readers::image::{image_info, ImageInfo, LARGE_IMAGE_PIXELS, LARGE_IMAGE_SIDE};
pub use readers::image_pdf::{images_to_pdf, ImageInput};
pub use readers::pdf_image::{PageRenderer, PdfToImageConverter, RenderedPage};
pub use registry::{
    convert, convert_files, convert_report, convert_report_with_progress, converter, read_document,
    registered, ConversionReport, Converter, OutputFile,
};
pub use traits::{Reader, Writer};

/// Product name, used in generated documents and window titles.
pub const APP_NAME: &str = "Verso";

/// Library version, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
