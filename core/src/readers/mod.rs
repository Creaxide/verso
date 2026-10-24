//! Shared helpers for readers.

pub mod csv;
pub mod docx;
pub mod html;
pub mod json;
pub mod markdown;
pub mod pdf;
pub mod rtf;
pub mod txt;

use crate::document::ImageRef;

/// Decode `data:` images; keep remote references as text-only references so a
/// conversion never touches the network.
pub(crate) fn image_ref(alt: &str, source: &str) -> ImageRef {
    let alt = alt.trim().to_string();
    if let Some((mime, data)) = decode_data_uri(source) {
        return ImageRef {
            alt,
            mime,
            data: Some(data),
            source: None,
        };
    }
    ImageRef {
        alt,
        mime: String::new(),
        data: None,
        source: Some(source.to_string()),
    }
}

fn decode_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    let rest = uri.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let mime = meta.trim_end_matches(";base64").to_string();
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    Some((mime, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_uri_images() {
        let image = image_ref("dot", "data:image/png;base64,aGVsbG8=");
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.data.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn keeps_remote_images_as_references() {
        let image = image_ref("photo", "https://example.com/a.png");
        assert!(image.data.is_none());
        assert_eq!(image.source.as_deref(), Some("https://example.com/a.png"));
    }
}
