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
}

impl Pair {
    pub fn is_possible(&self) -> bool {
        self.support.is_possible()
    }
}

/// The full 8 × 8 matrix, in [`Format::ALL`] order.
pub fn conversion_matrix() -> Vec<Pair> {
    let mut pairs = Vec::with_capacity(64);
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

    // One pair is genuinely impossible. Recovering table structure from a PDF
    // is guesswork; pretending otherwise is worse than saying no.
    if from == Format::Pdf && to == Format::Csv {
        return Pair {
            from,
            to,
            support: Support::Unsupported,
            notes: vec![
                "A PDF stores positioned text, not table structure, so columns cannot be recovered reliably.",
                "Use HTML, DOCX or CSV as the source instead.",
            ],
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
        assert_eq!(matrix.len(), 64);
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
    fn pdf_to_csv_is_the_only_unsupported_pair() {
        let unsupported: Vec<_> = conversion_matrix()
            .into_iter()
            .filter(|pair| !pair.is_possible())
            .map(|pair| (pair.from, pair.to))
            .collect();
        assert_eq!(unsupported, vec![(Format::Pdf, Format::Csv)]);
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
