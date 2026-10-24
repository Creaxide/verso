//! Plain text in and out.
//!
//! Plain text has no structure to lose, so the reader keeps blank-line
//! paragraphs and every line break inside a paragraph. The writer reverses that
//! exactly, which is what makes TXT → MD → TXT a clean round trip.

use crate::document::{Block, Document, Inline};
use crate::error::ConvertError;
use crate::options::Options;
use crate::traits::{Reader, Writer};
use crate::util::{normalize_newlines, plain_paragraphs};

pub struct TxtReader;
pub struct TxtWriter;

pub static READER: TxtReader = TxtReader;
pub static WRITER: TxtWriter = TxtWriter;

impl Reader for TxtReader {
    fn read(&self, input: &[u8], _options: &Options) -> Result<Document, ConvertError> {
        let text = String::from_utf8_lossy(input);
        let text = text.trim_start_matches('\u{feff}');
        let mut blocks = Vec::new();
        for paragraph in plain_paragraphs(text) {
            let mut inlines = Vec::new();
            for (index, line) in paragraph.split('\n').enumerate() {
                if index > 0 {
                    inlines.push(Inline::Break);
                }
                if !line.is_empty() {
                    inlines.push(Inline::Text(line.to_string()));
                }
            }
            if !inlines.is_empty() {
                blocks.push(Block::Paragraph(inlines));
            }
        }
        Ok(Document::new(blocks))
    }
}

impl Writer for TxtWriter {
    fn write(&self, document: &Document, _options: &Options) -> Result<Vec<u8>, ConvertError> {
        let mut out = String::new();
        for (index, block) in document.blocks.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            write_block(&mut out, block, 0);
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        Ok(out.into_bytes())
    }
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    let indent = "  ".repeat(depth);
    match block {
        Block::Heading { content, .. } => {
            out.push_str(&indent);
            out.push_str(&crate::document::inlines_plain(content));
            out.push('\n');
        }
        Block::Paragraph(content) => {
            for (index, line) in crate::document::inlines_plain(content)
                .split('\n')
                .enumerate()
            {
                if index > 0 {
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push_str(line);
            }
            out.push('\n');
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            for (index, item) in items.iter().enumerate() {
                let marker = if *ordered {
                    format!("{}.", start.saturating_add(index as u64))
                } else {
                    "-".to_string()
                };
                let mut body = String::new();
                for (block_index, child) in item.blocks.iter().enumerate() {
                    if block_index > 0 {
                        body.push('\n');
                    }
                    write_block(&mut body, child, depth + 1);
                }
                let body = body.trim_end_matches('\n');
                let mut lines = body.split('\n');
                if let Some(first) = lines.next() {
                    out.push_str(&indent);
                    out.push_str(&marker);
                    out.push(' ');
                    out.push_str(first.trim_start());
                    out.push('\n');
                }
                for line in lines {
                    out.push_str(&indent);
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        Block::Table(table) => {
            let text = crate::document::table_to_text(table);
            for line in text.lines() {
                out.push_str(&indent);
                out.push_str(line);
                out.push('\n');
            }
        }
        Block::Code { text, .. } => {
            for line in text.lines() {
                out.push_str(&indent);
                out.push_str(line);
                out.push('\n');
            }
        }
        Block::Quote(inner) => {
            let mut body = String::new();
            for child in inner {
                write_block(&mut body, child, depth);
            }
            for line in body.lines() {
                out.push_str(&indent);
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        Block::Image(image) => {
            out.push_str(&indent);
            out.push_str("[image: ");
            out.push_str(if image.alt.is_empty() {
                "untitled"
            } else {
                &image.alt
            });
            out.push_str("]\n");
        }
        Block::Rule => {
            out.push_str(&indent);
            out.push_str("----------------------------------------\n");
        }
    }
}

/// Normalize text for round-trip assertions.
pub fn normalized(text: &str) -> String {
    normalize_newlines(text)
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_blank_line_paragraphs() {
        let doc = READER.read(b"one\ntwo\n\nthree\n", &Options::default()).unwrap();
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.text_only(), "one\ntwo\nthree\n");
    }

    #[test]
    fn writes_blank_lines_between_blocks() {
        let doc = READER.read(b"one\n\nthree\n", &Options::default()).unwrap();
        let out = String::from_utf8(WRITER.write(&doc, &Options::default()).unwrap()).unwrap();
        assert_eq!(out, "one\n\nthree\n");
    }

    #[test]
    fn handles_bom_and_crlf() {
        let doc = READER
            .read("\u{feff}a\r\n\r\nb".as_bytes(), &Options::default())
            .unwrap();
        assert_eq!(doc.text_only(), "a\nb\n");
    }
}
