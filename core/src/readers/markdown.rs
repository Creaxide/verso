//! Markdown in and out, built on `pulldown-cmark`.

use pulldown_cmark::{
    Alignment as MdAlignment, CodeBlockKind, Event, HeadingLevel, Options as MdOptions, Parser,
    Tag, TagEnd,
};
use std::fmt::Write as _;

use crate::document::{
    inlines_plain, Align, Block, Document, ImageRef, Inline, Inlines, ListItem, Table,
};
use crate::error::ConvertError;
use crate::options::Options;
use crate::traits::{Reader, Writer};
use crate::util::escape_markdown;

pub struct MarkdownReader;
pub struct MarkdownWriter;

pub static READER: MarkdownReader = MarkdownReader;
pub static WRITER: MarkdownWriter = MarkdownWriter;

impl Reader for MarkdownReader {
    fn read(&self, input: &[u8], _options: &Options) -> Result<Document, ConvertError> {
        let text = String::from_utf8_lossy(input);
        let mut options = MdOptions::empty();
        options.insert(MdOptions::ENABLE_TABLES);
        options.insert(MdOptions::ENABLE_STRIKETHROUGH);
        options.insert(MdOptions::ENABLE_TASKLISTS);
        options.insert(MdOptions::ENABLE_HEADING_ATTRIBUTES);
        let parser = Parser::new_ext(text.trim_start_matches('\u{feff}'), options);
        let mut builder = Builder::default();
        for event in parser {
            builder.handle(event);
        }
        builder.finish()
    }
}

impl Writer for MarkdownWriter {
    fn write(&self, document: &Document, _options: &Options) -> Result<Vec<u8>, ConvertError> {
        let mut out = String::new();
        write_blocks(&mut out, &document.blocks, 0);
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        Ok(out.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

enum Frame {
    Paragraph {
        inlines: Inlines,
        heading: Option<u8>,
    },
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
        current: Vec<Block>,
    },
    Table {
        align: Vec<Align>,
        headers: Vec<Inlines>,
        rows: Vec<Vec<Inlines>>,
        row: Vec<Inlines>,
        cell: Inlines,
        in_head: bool,
    },
    Inline {
        kind: InlineKind,
        content: Inlines,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Image {
        alt: String,
        source: String,
    },
    RawHtml(String),
    Discard,
}

enum InlineKind {
    Strong,
    Emphasis,
    Strike,
    Link(String),
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    stack: Vec<Frame>,
    pending_images: Vec<ImageRef>,
    notices: Vec<String>,
}

impl Builder {
    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.inline(Inline::Code(code.to_string())),
            Event::SoftBreak => self.inline(Inline::Text(" ".to_string())),
            Event::HardBreak => self.inline(Inline::Break),
            Event::Rule => self.block(Block::Rule),
            Event::TaskListMarker(checked) => {
                self.inline(Inline::Text(if checked { "[x] " } else { "[ ] " }.to_string()));
            }
            Event::FootnoteReference(name) => {
                self.inline(Inline::Text(format!("[{name}]")));
            }
            Event::Html(html) => {
                if let Some(Frame::RawHtml(raw)) = self.stack.last_mut() {
                    raw.push_str(&html);
                } else {
                    self.notice("Raw HTML in the Markdown was ignored.");
                }
            }
            Event::InlineHtml(html) => {
                // Inline tags are dropped; their text content still arrives as Text.
                if !html.trim().is_empty() {
                    self.notice("Inline HTML in the Markdown was ignored.");
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.stack.push(Frame::Paragraph {
                inlines: Vec::new(),
                heading: None,
            }),
            Tag::Heading { level, .. } => self.stack.push(Frame::Paragraph {
                inlines: Vec::new(),
                heading: Some(heading_level(level)),
            }),
            Tag::BlockQuote(_) => self.stack.push(Frame::Quote(Vec::new())),
            Tag::List(start) => self.stack.push(Frame::List {
                ordered: start.is_some(),
                start: start.unwrap_or(1),
                items: Vec::new(),
                current: Vec::new(),
            }),
            Tag::Item => {
                if let Some(Frame::List { current, .. }) = self.stack.last_mut() {
                    current.clear();
                }
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let token = info.split_whitespace().next().unwrap_or("");
                        if token.is_empty() {
                            None
                        } else {
                            Some(token.to_string())
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                self.stack.push(Frame::Code {
                    language,
                    text: String::new(),
                });
            }
            Tag::Table(align) => self.stack.push(Frame::Table {
                align: align.into_iter().map(map_align).collect(),
                headers: Vec::new(),
                rows: Vec::new(),
                row: Vec::new(),
                cell: Vec::new(),
                in_head: false,
            }),
            Tag::TableHead => {
                if let Some(Frame::Table { in_head, row, .. }) = self.stack.last_mut() {
                    *in_head = true;
                    row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(Frame::Table { cell, .. }) = self.stack.last_mut() {
                    cell.clear();
                }
            }
            Tag::Emphasis => self.stack.push(Frame::Inline {
                kind: InlineKind::Emphasis,
                content: Vec::new(),
            }),
            Tag::Strong => self.stack.push(Frame::Inline {
                kind: InlineKind::Strong,
                content: Vec::new(),
            }),
            Tag::Strikethrough => self.stack.push(Frame::Inline {
                kind: InlineKind::Strike,
                content: Vec::new(),
            }),
            Tag::Link { dest_url, .. } => self.stack.push(Frame::Inline {
                kind: InlineKind::Link(dest_url.to_string()),
                content: Vec::new(),
            }),
            Tag::Image { dest_url, .. } => self.stack.push(Frame::Image {
                alt: String::new(),
                source: dest_url.to_string(),
            }),
            Tag::HtmlBlock => self.stack.push(Frame::RawHtml(String::new())),
            Tag::FootnoteDefinition(_) | Tag::MetadataBlock(_) => self.stack.push(Frame::Discard),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                let Some(Frame::Paragraph { inlines, heading }) = self.stack.pop() else {
                    return;
                };
                if inlines_plain(&inlines).trim().is_empty() {
                    self.flush_images();
                    return;
                }
                match heading {
                    Some(level) => self.block(Block::Heading {
                        level,
                        content: inlines,
                    }),
                    None => self.block(Block::Paragraph(inlines)),
                }
                self.flush_images();
            }
            TagEnd::Item => {
                if let Some(Frame::List { items, current, .. }) = self.stack.last_mut() {
                    let blocks = std::mem::take(current);
                    if !blocks.is_empty() {
                        items.push(ListItem { blocks });
                    }
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List {
                    ordered,
                    start,
                    items,
                    ..
                }) = self.stack.pop()
                {
                    if !items.is_empty() {
                        self.block(Block::List {
                            ordered,
                            start,
                            items,
                        });
                    }
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Quote(inner)) = self.stack.pop() {
                    if !inner.is_empty() {
                        self.block(Block::Quote(inner));
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { language, mut text }) = self.stack.pop() {
                    while text.ends_with('\n') || text.ends_with('\r') {
                        text.pop();
                    }
                    if let Some('\r') = text.chars().last() {
                        text.pop();
                    }
                    self.block(Block::Code { language, text });
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table {
                    align,
                    headers,
                    rows,
                    ..
                }) = self.stack.pop()
                {
                    self.block(Block::Table(Table {
                        headers,
                        rows,
                        align,
                    }));
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table {
                    headers, row, in_head, ..
                }) = self.stack.last_mut()
                {
                    *headers = std::mem::take(row);
                    *in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { rows, row, .. }) = self.stack.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::Table { row, cell, .. }) = self.stack.last_mut() {
                    row.push(std::mem::take(cell));
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                if let Some(Frame::Inline { kind, content }) = self.stack.pop() {
                    let inline = match kind {
                        InlineKind::Strong => Inline::Strong(content),
                        InlineKind::Emphasis => Inline::Emphasis(content),
                        InlineKind::Strike => Inline::Strike(content),
                        InlineKind::Link(href) => Inline::Link { content, href },
                    };
                    self.inline(inline);
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Image { alt, source }) = self.stack.pop() {
                    self.pending_images
                        .push(crate::readers::image_ref(&alt, &source));
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(Frame::RawHtml(raw)) = self.stack.pop() {
                    if !raw.trim().is_empty() {
                        match crate::readers::html::read_fragment(&raw) {
                            Ok(blocks) => {
                                for block in blocks {
                                    self.block(block);
                                }
                            }
                            Err(_) => self.notice("A raw HTML block could not be read and was skipped."),
                        }
                    }
                }
            }
            TagEnd::FootnoteDefinition | TagEnd::MetadataBlock(_) => {
                let _ = self.stack.pop();
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Code { text: buffer, .. } => {
                    buffer.push_str(text);
                    return;
                }
                Frame::Image { alt, .. } => {
                    alt.push_str(text);
                    return;
                }
                Frame::RawHtml(raw) => {
                    raw.push_str(text);
                    return;
                }
                Frame::Inline { content, .. } => {
                    content.push(Inline::Text(text.to_string()));
                    return;
                }
                Frame::Paragraph { inlines, .. } => {
                    inlines.push(Inline::Text(text.to_string()));
                    return;
                }
                Frame::Table { cell, .. } => {
                    cell.push(Inline::Text(text.to_string()));
                    return;
                }
                _ => {}
            }
        }
        self.block(Block::Paragraph(vec![Inline::Text(text.to_string())]));
    }

    fn inline(&mut self, inline: Inline) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Inline { content, .. } => {
                    content.push(inline);
                    return;
                }
                Frame::Paragraph { inlines, .. } => {
                    inlines.push(inline);
                    return;
                }
                Frame::Table { cell, .. } => {
                    cell.push(inline);
                    return;
                }
                Frame::List { current, .. } => {
                    current.push(Block::Paragraph(vec![inline]));
                    return;
                }
                Frame::Quote(blocks) => {
                    blocks.push(Block::Paragraph(vec![inline]));
                    return;
                }
                _ => {}
            }
        }
        self.blocks.push(Block::Paragraph(vec![inline]));
    }

    fn block(&mut self, block: Block) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Quote(blocks) => {
                    blocks.push(block);
                    return;
                }
                Frame::List { current, .. } => {
                    current.push(block);
                    return;
                }
                Frame::Paragraph { .. } | Frame::Table { .. } => {}
                _ => {}
            }
        }
        self.blocks.push(block);
    }

    fn flush_images(&mut self) {
        for image in self.pending_images.drain(..) {
            self.blocks.push(Block::Image(image));
        }
    }

    fn notice(&mut self, message: &str) {
        if !self.notices.iter().any(|existing| existing == message) {
            self.notices.push(message.to_string());
        }
    }

    fn finish(mut self) -> Result<Document, ConvertError> {
        while let Some(frame) = self.stack.pop() {
            match frame {
                Frame::Paragraph { inlines, heading } => {
                    if !inlines_plain(&inlines).trim().is_empty() {
                        match heading {
                            Some(level) => self.blocks.push(Block::Heading {
                                level,
                                content: inlines,
                            }),
                            None => self.blocks.push(Block::Paragraph(inlines)),
                        }
                    }
                }
                Frame::Quote(blocks) => self.blocks.push(Block::Quote(blocks)),
                Frame::List {
                    ordered,
                    start,
                    items,
                    ..
                } => self.blocks.push(Block::List {
                    ordered,
                    start,
                    items,
                }),
                _ => {}
            }
        }
        Ok(Document {
            title: None,
            blocks: self.blocks,
            notices: self.notices,
        })
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn map_align(align: MdAlignment) -> Align {
    match align {
        MdAlignment::Center => Align::Center,
        MdAlignment::Right => Align::Right,
        _ => Align::Left,
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

fn write_blocks(out: &mut String, blocks: &[Block], depth: usize) {
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        write_block(out, block, depth);
    }
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    match block {
        Block::Heading { level, content } => {
            let hashes = "#".repeat((*level).clamp(1, 6) as usize);
            let _ = writeln!(out, "{hashes} {}", write_inlines(content, false));
        }
        Block::Paragraph(content) => {
            let marker = if depth > 0 { "" } else { "" };
            let _ = writeln!(out, "{marker}{}", write_inlines(content, false));
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
                write_blocks(&mut body, &item.blocks, depth + 1);
                let body = body.trim_end_matches('\n');
                let mut lines = body.split('\n');
                if let Some(first) = lines.next() {
                    let _ = writeln!(out, "{}{marker} {}", "  ".repeat(depth), first);
                }
                for line in lines {
                    if line.is_empty() {
                        out.push('\n');
                    } else {
                        let _ = writeln!(out, "{}{}", "  ".repeat(depth + 1), line.trim_start());
                    }
                }
            }
        }
        Block::Table(table) => {
            write_table(out, table);
        }
        Block::Code { language, text } => {
            let fence = fence_for(text);
            let language = language.as_deref().unwrap_or("");
            let _ = writeln!(out, "{fence}{language}");
            out.push_str(text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
            let _ = writeln!(out, "{fence}");
        }
        Block::Quote(inner) => {
            let mut body = String::new();
            write_blocks(&mut body, inner, depth);
            for line in body.trim_end_matches('\n').lines() {
                if line.is_empty() {
                    out.push_str(">\n");
                } else {
                    let _ = writeln!(out, "> {line}");
                }
            }
        }
        Block::Image(image) => {
            let target = image_target(image);
            let alt = escape_markdown(&image.alt);
            if target.is_empty() {
                let _ = writeln!(
                    out,
                    "*[image: {}]*",
                    if alt.is_empty() { "untitled" } else { &alt }
                );
            } else {
                let _ = writeln!(out, "![{alt}]({target})");
            }
        }
        Block::Rule => out.push_str("---\n"),
    }
}

fn write_table(out: &mut String, table: &Table) {
    if table.is_empty() {
        return;
    }
    let columns = table.column_count().max(1);
    let header = |index: usize| {
        let cell = table
            .headers
            .get(index)
            .map(|cell| write_inlines(cell, true))
            .unwrap_or_default();
        cell.replace('|', "\\|")
    };
    let header_cells: Vec<String> = (0..columns).map(header).collect();
    let _ = writeln!(out, "| {} |", header_cells.join(" | "));
    let separators: Vec<String> = (0..columns)
        .map(|index| match table.align_at(index) {
            Align::Left => ":---".to_string(),
            Align::Center => ":---:".to_string(),
            Align::Right => "---:".to_string(),
        })
        .collect();
    let _ = writeln!(out, "| {} |", separators.join(" | "));
    for row in &table.rows {
        let cells: Vec<String> = (0..columns)
            .map(|index| {
                row.get(index)
                    .map(|cell| write_inlines(cell, true).replace('|', "\\|"))
                    .unwrap_or_default()
            })
            .collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }
}

fn write_inlines(inlines: &[Inline], in_table: bool) -> String {
    let mut out = String::new();
    write_inline_list(&mut out, inlines, in_table);
    out
}

fn write_inline_list(out: &mut String, inlines: &[Inline], in_table: bool) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&escape_markdown(text)),
            Inline::Strong(content) => {
                let _ = write!(out, "**{}**", write_inlines(content, in_table));
            }
            Inline::Emphasis(content) => {
                let _ = write!(out, "*{}*", write_inlines(content, in_table));
            }
            Inline::Strike(content) => {
                let _ = write!(out, "~~{}~~", write_inlines(content, in_table));
            }
            Inline::Code(code) => {
                let fence = inline_code_fence(code);
                let _ = write!(out, "{fence}{code}{fence}");
            }
            Inline::Link { content, href } => {
                let label = write_inlines(content, in_table);
                if href.contains([' ', '(', ')']) {
                    let _ = write!(out, "[{label}](<{href}>)");
                } else {
                    let _ = write!(out, "[{label}]({href})");
                }
            }
            Inline::Break => {
                if in_table {
                    out.push(' ');
                } else {
                    out.push_str("\\\n");
                }
            }
        }
    }
}

fn image_target(image: &ImageRef) -> String {
    if let Some(data) = &image.data {
        if !image.mime.is_empty() {
            use base64::Engine as _;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            return format!("data:{};base64,{}", image.mime, encoded);
        }
    }
    image.source.clone().unwrap_or_else(|| "".to_string())
}

fn fence_for(text: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for character in text.chars() {
        if character == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

fn inline_code_fence(text: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for character in text.chars() {
        if character == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(0) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Document {
        READER.read(text.as_bytes(), &Options::default()).unwrap()
    }

    #[test]
    fn reads_headings_emphasis_and_links() {
        let doc = read("# Title\n\nHello **bold** and *soft* [link](https://example.com).\n");
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
        let Block::Paragraph(inlines) = &doc.blocks[1] else {
            panic!("expected paragraph");
        };
        assert!(inlines.iter().any(|inline| matches!(inline, Inline::Strong(_))));
        assert!(inlines.iter().any(|inline| matches!(inline, Inline::Emphasis(_))));
        assert!(inlines.iter().any(|inline| matches!(inline, Inline::Link { .. })));
    }

    #[test]
    fn reads_nested_lists() {
        let doc = read("- one\n- two\n  - two a\n");
        let Block::List { items, .. } = &doc.blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(items.len(), 2);
        assert!(items[1].blocks.iter().any(|block| matches!(block, Block::List { .. })));
    }

    #[test]
    fn reads_tables() {
        let doc = read("| a | b |\n|:--|--:|\n| 1 | 2 |\n");
        let Block::Table(table) = &doc.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.align_at(1), Align::Right);
    }

    #[test]
    fn reads_code_blocks_with_language() {
        let doc = read("```rust\nlet x = 1;\n```\n");
        let Block::Code { language, text } = &doc.blocks[0] else {
            panic!("expected code");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "let x = 1;");
    }

    #[test]
    fn writes_and_reads_again() {
        let doc = Document::new(vec![
            Block::Heading {
                level: 2,
                content: vec![Inline::Text("Head".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("A ".into()),
                Inline::Strong(vec![Inline::Text("bold".into())]),
                Inline::Text(" claim.".into()),
            ]),
        ]);
        let md = String::from_utf8(WRITER.write(&doc, &Options::default()).unwrap()).unwrap();
        let back = read(&md);
        assert_eq!(back.text_only(), "Head\nA bold claim.\n");
    }

    #[test]
    fn escapes_structure_in_prose() {
        let doc = Document::new(vec![Block::Paragraph(vec![Inline::Text(
            "# not a heading".into(),
        )])]);
        let md = String::from_utf8(WRITER.write(&doc, &Options::default()).unwrap()).unwrap();
        assert!(md.contains("\\#"));
    }
}
