//! Reader and writer contracts.
//!
//! A `Converter` is a source/target pair. Internally it is one `Reader` and one
//! `Writer` sharing the intermediate [`crate::Document`], which is why adding a
//! format costs two implementations instead of eight.

use crate::document::Document;
use crate::error::ConvertError;
use crate::options::Options;

pub trait Reader: Send + Sync {
    fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError>;
}

pub trait Writer: Send + Sync {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError>;
}
