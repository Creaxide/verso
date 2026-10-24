//! RTF in and out.
//!
//! RTF is a presentation format. It knows bold and italic, not headings or
//! lists. The reader therefore recovers paragraphs and character formatting
//! only; the writer bakes document structure into spacing and styling, which is
//! exactly the trade the conversion matrix warns about.

use crate::document::{inlines_plain, table_to_text, Block, Document, Inline, Inlines};
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::traits::{Reader, Writer};
use crate::util::{escape_rtf, winansi_char};

pub struct RtfReader;
pub struct RtfWriter;

pub static READER: RtfReader = RtfReader;
pub static WRITER: RtfWriter = RtfWriter;

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
}

impl Style {
    fn wrap(self, run: Inlines) -> Inlines {
        let mut run = run;
        if self.strike {
            run = vec![Inline::Strike(run)];
        }
        if self.italic {
            run = vec![Inline::Emphasis(run)];
        }
        if self.bold {
            run = vec![Inline::Strong(run)];
        }
        run
    }
}

#[derive(Clone, Copy, Debug)]
struct State {
    style: Style,
    /// Inside a destination group (`{\fonttbl ...}`), whose text is not content.
    skip: bool,
    /// How many fallback characters follow a `\uN` escape.
    fallback: usize,
    /// Value of the last `\uc` control word.
    uc: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            style: Style::default(),
            skip: false,
            fallback: 0,
            uc: 1,
        }
    }
}

impl Reader for RtfReader {
    fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError> {
        options.check_size(input)?;
        let bytes = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
        let start = bytes
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(0);
        if !bytes[start..].starts_with(b"{\\rtf") {
            return Err(ConvertError::NotThisFormat {
                expected: Format::Rtf,
            });
        }

        let mut collector = Collector::default();
        let mut state = State::default();
        let mut stack: Vec<State> = Vec::new();
        let mut index = start;

        while index < bytes.len() {
            match bytes[index] {
                b'{' => {
                    stack.push(state);
                    index += 1;
                }
                b'}' => {
                    if let Some(previous) = stack.pop() {
                        state = previous;
                    }
                    index += 1;
                }
                b'\\' => {
                    index = control(bytes, index + 1, &mut state, &mut collector);
                }
                // A raw line break in the file carries no meaning.
                b'\r' | b'\n' => index += 1,
                byte => {
                    index += 1;
                    if state.skip {
                        continue;
                    }
                    if state.fallback > 0 {
                        state.fallback -= 1;
                        continue;
                    }
                    collector.push(if byte < 0x80 {
                        byte as char
                    } else {
                        winansi_char(byte)
                    });
                }
            }
        }
        collector.end_paragraph();

        if collector.blocks.is_empty() {
            return Err(ConvertError::EmptyDocument);
        }
        Ok(Document::new(collector.blocks))
    }
}

/// Parse one control word or control symbol beginning at `start` (the byte after
/// the backslash) and return the index just past it.
fn control(bytes: &[u8], start: usize, state: &mut State, collector: &mut Collector) -> usize {
    let Some(&next) = bytes.get(start) else {
        return start;
    };

    match next {
        b'\'' => {
            let mut index = start + 1;
            let mut value: u32 = 0;
            let mut digits = 0;
            while digits < 2 {
                let Some(&byte) = bytes.get(index) else {
                    break;
                };
                let Some(digit) = (byte as char).to_digit(16) else {
                    break;
                };
                value = value * 16 + digit;
                index += 1;
                digits += 1;
            }
            if digits < 2 {
                return index;
            }
            if !state.skip && state.fallback == 0 {
                collector.push(winansi_char(value as u8));
            } else if state.fallback > 0 {
                state.fallback -= 1;
            }
            index
        }
        b'*' => {
            // An ignorable destination: everything up to the group's closing
            // brace is metadata, not content.
            state.skip = true;
            start + 1
        }
        b'\\' | b'{' | b'}' => {
            if !state.skip {
                collector.push(next as char);
            }
            start + 1
        }
        b'~' => {
            if !state.skip {
                collector.push('\u{a0}');
            }
            start + 1
        }
        b'\r' | b'\n' => start + 1,
        b'-' => start + 1,
        b'_' => {
            if !state.skip {
                collector.push('-');
            }
            start + 1
        }
        _ if next.is_ascii_alphabetic() => {
            let mut index = start;
            while matches!(bytes.get(index), Some(byte) if byte.is_ascii_alphabetic()) {
                index += 1;
            }
            let word = String::from_utf8_lossy(&bytes[start..index]).to_ascii_lowercase();

            let mut negative = false;
            if bytes.get(index) == Some(&b'-') {
                negative = true;
                index += 1;
            }
            let digits_start = index;
            while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
                index += 1;
            }
            let parameter = if index > digits_start {
                bytes[digits_start..index]
                    .iter()
                    .fold(Some(0i32), |accumulator, byte| {
                        accumulator.and_then(|value| {
                            let digit = i32::from(byte - b'0');
                            value.checked_mul(10)?.checked_add(digit)
                        })
                    })
                    .map(|value| if negative { -value } else { value })
            } else {
                None
            };
            // A single space terminates a control word and is not content.
            if bytes.get(index) == Some(&b' ') {
                index += 1;
            }

            if is_destination(&word) {
                state.skip = true;
                return index;
            }
            if word == "bin" {
                let count = parameter.unwrap_or(0).max(0) as usize;
                return index.saturating_add(count).min(bytes.len());
            }
            if state.skip {
                return index;
            }

            match word.as_str() {
                "par" => collector.end_paragraph(),
                "line" => collector.line_break(),
                "tab" => collector.push('\t'),
                "u" => {
                    if let Some(value) = parameter {
                        let code = (value as i64 & 0xFFFF) as u32;
                        if let Some(character) = char::from_u32(code) {
                            collector.push(character);
                        }
                        state.fallback = state.uc;
                    }
                }
                "uc" => {
                    if let Some(value) = parameter {
                        state.uc = value.clamp(0, 10) as usize;
                    }
                }
                "b" => {
                    state.style.bold = parameter.map_or(true, |value| value != 0);
                    collector.set_style(state.style);
                }
                "i" => {
                    state.style.italic = parameter.map_or(true, |value| value != 0);
                    collector.set_style(state.style);
                }
                "strike" | "striked" => {
                    state.style.strike = parameter.map_or(true, |value| value != 0);
                    collector.set_style(state.style);
                }
                "plain" => {
                    state.style = Style::default();
                    collector.set_style(state.style);
                }
                "bullet" => collector.push('\u{2022}'),
                "emdash" => collector.push('\u{2014}'),
                "endash" => collector.push('\u{2013}'),
                "lquote" => collector.push('\u{2018}'),
                "rquote" => collector.push('\u{2019}'),
                "ldblquote" => collector.push('\u{201C}'),
                "rdblquote" => collector.push('\u{201D}'),
                _ => {}
            }
            index
        }
        _ => start + 1,
    }
}

/// Control words whose groups hold metadata, field instructions or binary
/// payloads rather than document text.
fn is_destination(word: &str) -> bool {
    matches!(
        word,
        "fonttbl"
            | "colortbl"
            | "stylesheet"
            | "info"
            | "pict"
            | "nonshppict"
            | "shppict"
            | "header"
            | "headerl"
            | "headerr"
            | "headerf"
            | "footer"
            | "footerl"
            | "footerr"
            | "footerf"
            | "footnote"
            | "annotation"
            | "object"
            | "objdata"
            | "themedata"
            | "colorschememapping"
            | "datastore"
            | "latentstyles"
            | "listtable"
            | "listoverridetable"
            | "revtbl"
            | "rsidtbl"
            | "generator"
            | "xmlnstbl"
            | "filetbl"
            | "mmathpr"
            | "fldinst"
            | "bkmkstart"
            | "bkmkend"
            | "pntext"
            | "pntxta"
            | "pntxtb"
    )
}

#[derive(Default)]
struct Collector {
    blocks: Vec<Block>,
    current: Inlines,
    buffer: String,
    style: Style,
}

impl Collector {
    fn push(&mut self, character: char) {
        self.buffer.push(character);
    }

    fn set_style(&mut self, style: Style) {
        if self.style != style {
            self.flush();
            self.style = style;
        }
    }

    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buffer);
        self.current
            .extend(self.style.wrap(vec![Inline::Text(text)]));
    }

    fn line_break(&mut self) {
        self.flush();
        self.current.push(Inline::Break);
    }

    fn end_paragraph(&mut self) {
        self.flush();
        trim_inlines(&mut self.current);
        while matches!(self.current.last(), Some(Inline::Break)) {
            self.current.pop();
        }
        if !self.current.is_empty() {
            self.blocks
                .push(Block::Paragraph(std::mem::take(&mut self.current)));
        }
        self.current = Vec::new();
    }
}

fn trim_inlines(inlines: &mut Inlines) {
    while let Some(first) = inlines.first_mut() {
        match first {
            Inline::Text(text) => {
                let trimmed = text.trim_start().to_string();
                if trimmed.is_empty() {
                    inlines.remove(0);
                } else {
                    *text = trimmed;
                    break;
                }
            }
            Inline::Break => {
                inlines.remove(0);
            }
            _ => break,
        }
    }
    loop {
        let Some(last) = inlines.last_mut() else {
            break;
        };
        match last {
            Inline::Text(text) => {
                let trimmed = text.trim_end().to_string();
                if trimmed.is_empty() {
                    inlines.pop();
                } else {
                    *text = trimmed;
                    break;
                }
            }
            Inline::Break => {
                inlines.pop();
            }
            _ => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

impl Writer for RtfWriter {
    fn write(&self, document: &Document, _options: &Options) -> Result<Vec<u8>, ConvertError> {
        let mut out = String::with_capacity(1024);
        out.push_str("{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1\n");
        out.push_str("{\\fonttbl{\\f0\\fswiss Helvetica;}{\\f1\\fmodern Courier New;}}\n");
        out.push_str("{\\colortbl;\\red0\\green0\\blue0;\\red112\\green112\\blue112;}\n");
        out.push_str("\\viewkind4\\pard\\f0\\fs22\n");
        write_blocks(&mut out, &document.blocks);
        out.push_str("}\n");
        Ok(out.into_bytes())
    }
}

fn write_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        write_block(out, block);
    }
}

fn write_block(out: &mut String, block: &Block) {
    match block {
        Block::Heading { level, content } => {
            let size = match level {
                1 => 36,
                2 => 30,
                3 => 26,
                _ => 24,
            };
            out.push_str("\\pard\\sb240\\sa120{\\b\\fs");
            out.push_str(&size.to_string());
            out.push(' ');
            write_inlines(out, content);
            out.push_str("}\\fs22\\par\n");
        }
        Block::Paragraph(content) => {
            out.push_str("\\pard\\sa120\\fs22 ");
            write_inlines(out, content);
            out.push_str("\\par\n");
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            for (index, item) in items.iter().enumerate() {
                out.push_str("\\pard\\li360\\fi-360\\tx360 ");
                if *ordered {
                    out.push_str(&escape_rtf(&format!(
                        "{}.",
                        start.saturating_add(index as u64)
                    )));
                } else {
                    out.push_str("\\bullet");
                }
                out.push_str("\\tab ");
                for (child_index, child) in item.blocks.iter().enumerate() {
                    if child_index > 0 {
                        out.push_str("\\line ");
                    }
                    out.push_str(&escape_rtf(&block_plain(child)));
                }
                out.push_str("\\par\n");
            }
        }
        Block::Table(table) => {
            if !table.headers.is_empty() {
                out.push_str("\\pard\\sa60{\\b ");
                write_cells(out, &table.headers);
                out.push_str("}\\par\n");
            }
            for row in &table.rows {
                out.push_str("\\pard\\sa120\\fs22 ");
                write_cells(out, row);
                out.push_str("\\par\n");
            }
        }
        Block::Code { text, .. } => {
            out.push_str("\\pard\\f1\\fs18\\sa120 ");
            out.push_str(&escape_rtf(text.trim_end_matches('\n')));
            out.push_str("\\f0\\fs22\\par\n");
        }
        Block::Quote(inner) => {
            out.push_str("\\pard\\li720\\ri720\\sa120{\\i ");
            let text = inner
                .iter()
                .map(block_plain)
                .collect::<Vec<_>>()
                .join("\n");
            out.push_str(&escape_rtf(&text));
            out.push_str("}\\par\n");
        }
        Block::Image(image) => {
            out.push_str("\\pard\\sa120{\\i\\cf2 [image: ");
            out.push_str(&escape_rtf(if image.alt.trim().is_empty() {
                "untitled"
            } else {
                image.alt.trim()
            }));
            out.push_str("]}\\cf0\\par\n");
        }
        Block::Rule => {
            out.push_str("\\pard\\qc\\sa120{\\cf2 ----------------------------------------}\\cf0\\par\n");
        }
    }
}

fn write_cells(out: &mut String, cells: &[Inlines]) {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            out.push_str("\\tab ");
        }
        write_inlines(out, cell);
    }
}

fn write_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&escape_rtf(text)),
            Inline::Strong(inner) => {
                out.push_str("{\\b ");
                write_inlines(out, inner);
                out.push('}');
            }
            Inline::Emphasis(inner) => {
                out.push_str("{\\i ");
                write_inlines(out, inner);
                out.push('}');
            }
            Inline::Strike(inner) => {
                out.push_str("{\\strike ");
                write_inlines(out, inner);
                out.push('}');
            }
            Inline::Code(code) => {
                out.push_str("{\\f1 ");
                out.push_str(&escape_rtf(code));
                out.push('}');
            }
            Inline::Link { content, href } => {
                out.push_str("{\\field{\\*\\fldinst{HYPERLINK \"");
                out.push_str(&escape_rtf(href));
                out.push_str("\"}}{\\fldrslt{");
                write_inlines(out, content);
                out.push_str("}}}");
            }
            Inline::Break => out.push_str("\\line "),
        }
    }
}

/// Text of any block without paragraph properties, used inside list items,
/// quotes and other containers.
fn block_plain(block: &Block) -> String {
    match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => inlines_plain(content),
        Block::List {
            ordered,
            start,
            items,
        } => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if *ordered {
                    format!("{}.", start.saturating_add(index as u64))
                } else {
                    "\u{2022}".to_string()
                };
                let body = item
                    .blocks
                    .iter()
                    .map(block_plain)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{marker} {body}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table(table) => table_to_text(table).trim_end().to_string(),
        Block::Code { text, .. } => text.trim_end().to_string(),
        Block::Quote(inner) => inner
            .iter()
            .map(block_plain)
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Image(image) => format!(
            "[image: {}]",
            if image.alt.trim().is_empty() {
                "untitled"
            } else {
                image.alt.trim()
            }
        ),
        Block::Rule => "----------------------------------------".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Document {
        READER
            .read(text.as_bytes(), &Options::default())
            .expect("valid RTF")
    }

    #[test]
    fn reads_paragraphs_and_formatting() {
        let document = read(
            r"{\rtf1\ansi\deff0{\fonttbl{\f0 Helvetica;}}{\colortbl;\red0\green0\blue0;}
               \pard Hello {\b bold} and {\i italic}.\par Second paragraph.\par}",
        );
        assert_eq!(document.blocks.len(), 2);
        assert_eq!(document.text_only(), "Hello bold and italic.\nSecond paragraph.\n");
        assert!(matches!(
            document.blocks.first(),
            Some(Block::Paragraph(inlines)) if inlines.iter().any(|i| matches!(i, Inline::Strong(_)))
        ));
    }

    #[test]
    fn decodes_unicode_and_code_page_escapes() {
        let document = read(r"{\rtf1\ansi\uc1 Caf\'e9 \u26412?\u26085?}");
        assert_eq!(document.text_only(), "Café 本日\n");
    }

    #[test]
    fn skips_metadata_groups() {
        let document = read(
            r"{\rtf1\ansi{\info{\title Hidden}}\pard Visible\par{\*\generator x;}}",
        );
        assert_eq!(document.text_only(), "Visible\n");
    }

    #[test]
    fn rejects_other_formats() {
        let error = READER
            .read(b"plain text", &Options::default())
            .expect_err("not RTF");
        assert_eq!(error.code(), "wrong-format");
    }

    #[test]
    fn writer_output_is_readable_by_its_reader() {
        let source = Document::new(vec![
            Block::Heading {
                level: 2,
                content: vec![Inline::Text("Report".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("Some ".into()),
                Inline::Strong(vec![Inline::Text("bold".into())]),
                Inline::Text(" text.".into()),
            ]),
            Block::List {
                ordered: false,
                start: 1,
                items: vec![
                    crate::document::ListItem::paragraph("first"),
                    crate::document::ListItem::paragraph("second"),
                ],
            },
        ]);
        let bytes = WRITER.write(&source, &Options::default()).expect("writes");
        let text = String::from_utf8(bytes.clone()).expect("UTF-8 RTF");
        assert!(text.starts_with("{\\rtf1"));
        let round_trip = READER.read(&bytes, &Options::default()).expect("reads");
        let plain = round_trip.text_only();
        assert!(plain.contains("Report"));
        assert!(plain.contains("Some bold text."));
        assert!(plain.contains("first"));
        assert!(plain.contains("second"));
    }

    #[test]
    fn writes_links_as_fields() {
        let source = Document::new(vec![Block::Paragraph(vec![Inline::Link {
            content: vec![Inline::Text("site".into())],
            href: "https://example.com".into(),
        }])]);
        let bytes = WRITER.write(&source, &Options::default()).expect("writes");
        let text = String::from_utf8(bytes).expect("UTF-8 RTF");
        assert!(text.contains("HYPERLINK"));
        assert!(text.contains("https://example.com"));
    }
}
