//! The conversion matrix: which pairs are possible, and what they cost.

use crate::format::Format;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Support {
    /// No structural loss specific to the pair.
    Native,
    /// Possible, with a loss the UI states before converting.
    Lossy,
    /// Not possible; the UI disables the action and explains why.
    Unsupported,
}

impl Support {
    pub const fn is_possible(self) -> bool {
        !matches!(self, Support::Unsupported)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Support::Native => "clean",
            Support::Lossy => "lossy",
            Support::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub from: Format,
    pub to: Format,
    pub support: Support,
    /// Risks shown before conversion, in plain language.
    pub notes: Vec<&'static str>,
    /// True when the produced output can be turned back into the source,
    /// possibly with a confirmation step (multi-file output).
    pub reversible: bool,
    /// True when this pair writes more than one file (PDF → images).
    pub multi_file_output: bool,
}

impl Pair {
    pub fn is_possible(&self) -> bool {
        self.support.is_possible()
    }
}

/// The full 13 × 13 matrix, in [`Format::ALL`] order.
pub fn conversion_matrix() -> Vec<Pair> {
    let mut pairs = Vec::with_capacity(Format::ALL.len() * Format::ALL.len());
    for from in Format::ALL {
        for to in Format::ALL {
            pairs.push(pair_info(from, to));
        }
    }
    pairs
}

pub fn pair_info(from: Format, to: Format) -> Pair {
    let mut notes: Vec<&'static str> = Vec::new();
    let mut support = Support::Native;

    // One document pair is genuinely impossible. Recovering table structure
    // from a PDF is guesswork; pretending otherwise is worse than saying no.
    if from == Format::Pdf && to == Format::Csv {
        return Pair {
            from,
            to,
            support: Support::Unsupported,
            notes: vec![
                "A PDF stores positioned text, not table structure, so columns cannot be recovered reliably.",
                "Use HTML, DOCX or CSV as the source instead.",
            ],
            reversible: false,
            multi_file_output: false,
        };
    }

    // PDF pages become raster images; one output per page.
    if from == Format::Pdf && matches!(to, Format::Png | Format::Jpg) {
        return Pair {
            from,
            to,
            support: Support::Lossy,
            notes: vec![
                "Each page is rasterized at the chosen DPI; text stops being selectable and searchable.",
                "A multi-page PDF writes one file per page, or a single ZIP.",
                "A scanned or damaged PDF is refused with an explanation rather than written empty.",
            ],
            reversible: true,
            multi_file_output: true,
        };
    }

    // Images become PDF pages; this is the reverse of PDF → image.
    if from.is_image() && to == Format::Pdf {
        return Pair {
            from,
            to,
            support: Support::Lossy,
            notes: vec![
                "One image per page. JPEG data is embedded directly when no resize is requested.",
                "Page size can match the image, or fit A4/Letter/Legal with margins.",
                "Transparency is composited over the white page.",
            ],
            // PDF → WebP/BMP/GIF does not exist, so only PNG/JPG can turn back.
            reversible: matches!(from, Format::Png | Format::Jpg),
            multi_file_output: false,
        };
    }

    // Stage 1 supports image ↔ image only. Document → image and image →
    // document pairs are named here so the matrix never promises a converter
    // that is not registered.
    if from.is_image() && !to.is_image() {
        return Pair {
            from,
            to,
            support: Support::Unsupported,
            notes: vec![
                "Images become images in this build. Embedding an image in a document is not available yet.",
                "Choose PNG, JPG, WEBP, BMP or GIF as the target.",
            ],
            reversible: false,
            multi_file_output: false,
        };
    }
    if !from.is_image() && to.is_image() {
        return Pair {
            from,
            to,
            support: Support::Unsupported,
            notes: vec![
                "Document pages cannot be rasterized in this build.",
                "Convert the document to PDF first; PDF → image arrives in the next stage.",
            ],
            reversible: false,
            multi_file_output: false,
        };
    }

    if from.is_image() && to.is_image() {
        if from == Format::Gif && to != Format::Gif {
            support = Support::Lossy;
            notes.push("Only the first frame of a GIF is used; animation is not preserved.");
        }
        if from.has_alpha() && !to.has_alpha() {
            support = Support::Lossy;
            notes.push("Transparency is flattened onto white because the target has no alpha channel.");
        }
        if to == Format::Gif {
            support = Support::Lossy;
            notes.push("GIF output is limited to a 256-colour palette.");
        }
        if from == to && matches!(to, Format::Jpg | Format::Webp) {
            support = Support::Lossy;
            notes.push("Re-encoding adds another generation of compression.");
        }
        return Pair {
            from,
            to,
            support,
            notes,
            reversible: true,
            multi_file_output: false,
        };
    }

    if from == Format::Pdf && to != Format::Pdf {
        support = Support::Lossy;
        notes.push("A PDF has no headings, lists or tables — reading it recovers plain paragraphs in page order.");
        notes.push("Text in scans or as images is not readable; scanned PDFs are refused rather than exported empty.");
    }

    if to == Format::Pdf && from != Format::Pdf {
        support = Support::Lossy;
        notes.push("PDF reflows the document; the original page layout is not reproduced.");
        notes.push("Images are left out of PDF output.");
        notes.push("Characters outside Windows-1252 are replaced with ?.");
    }

    if to == Format::Csv && from != Format::Csv {
        support = Support::Lossy;
        notes.push("Only the first table is exported; a document with no table becomes a single column.");
    }

    if to == Format::Rtf && !matches!(from, Format::Rtf | Format::Txt) {
        support = Support::Lossy;
        notes.push("RTF writes presentation, not structure: tables become tab-separated lines and headings become styled paragraphs.");
    }

    if from == Format::Rtf && matches!(to, Format::Markdown | Format::Html | Format::Docx | Format::Pdf) {
        support = Support::Lossy;
        notes.push("RTF has no heading or list semantics, so those become plain paragraphs.");
    }

    if from == Format::Json && to == Format::Csv {
        support = Support::Lossy;
        notes.push("Exports an array of flat objects; any other JSON becomes a single cell.");
    }

    Pair {
        from,
        to,
        support,
        notes,
        // PDF → CSV is the one refused reverse; everything else can be turned.
        reversible: !(from == Format::Csv && to == Format::Pdf),
        multi_file_output: false,
    }
}

/// True when the pair can be converted (lossy or not).
pub fn is_supported(from: Format, to: Format) -> bool {
    pair_info(from, to).support.is_possible()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_has_every_pair_once() {
        let matrix = conversion_matrix();
        assert_eq!(matrix.len(), Format::ALL.len() * Format::ALL.len());
        for from in Format::ALL {
            for to in Format::ALL {
                assert_eq!(
                    matrix.iter().filter(|p| p.from == from && p.to == to).count(),
                    1
                );
            }
        }
    }

    #[test]
    fn pdf_to_csv_is_still_refused() {
        let error = pair_info(Format::Pdf, Format::Csv);
        assert_eq!(error.support, Support::Unsupported);
        assert!(!error.notes.is_empty());
    }

    #[test]
    fn image_and_document_pairs_are_limited_to_pdf() {
        for from in Format::ALL {
            for to in Format::ALL {
                if from.is_image() != to.is_image() {
                    let expected = (from == Format::Pdf
                        && matches!(to, Format::Png | Format::Jpg))
                        || (from.is_image() && to == Format::Pdf);
                    assert_eq!(
                        pair_info(from, to).is_possible(),
                        expected,
                        "{from:?} → {to:?} support is wrong"
                    );
                }
            }
        }
    }

    #[test]
    fn pdf_to_images_is_marked_multi_file() {
        for to in [Format::Png, Format::Jpg] {
            let pair = pair_info(Format::Pdf, to);
            assert!(pair.is_possible());
            assert!(pair.multi_file_output, "PDF → {to:?} should be multi-file");
            assert!(pair.reversible);
        }
    }

    #[test]
    fn reversibility_flag_matches_the_reverse_pair() {
        for pair in conversion_matrix() {
            let reverse_possible = pair_info(pair.to, pair.from).is_possible();
            if pair.reversible {
                assert!(
                    pair.is_possible(),
                    "{} → {} is reversible but unsupported",
                    pair.from,
                    pair.to
                );
                assert!(
                    reverse_possible,
                    "{} → {} is marked reversible but its reverse is not",
                    pair.from,
                    pair.to
                );
            }
            if pair.is_possible() && reverse_possible {
                assert!(
                    pair.reversible,
                    "{} → {} has a supported reverse but is marked non-reversible",
                    pair.from,
                    pair.to
                );
            }
        }
    }

    #[test]
    fn image_pairs_are_all_registered() {
        for from in Format::ALL.into_iter().filter(|format| format.is_image()) {
            for to in Format::ALL.into_iter().filter(|format| format.is_image()) {
                assert!(is_supported(from, to), "{from:?} → {to:?} should be supported");
            }
        }
    }

    #[test]
    fn lossy_pairs_carry_a_reason() {
        for pair in conversion_matrix() {
            if pair.support == Support::Lossy {
                assert!(!pair.notes.is_empty(), "{} → {} has no note", pair.from, pair.to);
            }
            if pair.support == Support::Native {
                assert!(pair.notes.is_empty(), "{} → {} is native but has notes", pair.from, pair.to);
            }
        }
    }

    #[test]
    fn text_to_text_is_clean() {
        assert_eq!(pair_info(Format::Txt, Format::Markdown).support, Support::Native);
        assert_eq!(pair_info(Format::Markdown, Format::Txt).support, Support::Native);
    }
}
