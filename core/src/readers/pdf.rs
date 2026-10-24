//! PDF writing by hand.
//!
//! A PDF is a small object graph plus a content stream. Writing it directly
//! keeps the binary small, keeps output deterministic, and gives exact control
//! over how a document model reflows onto pages. The cost is that text is
//! limited to the Base-14 fonts and Windows-1252; the UI states both limits.
//!
//! Reading lives in `readers/pdf.rs`.

use std::fmt::Write as _;
use std::io::Write as _;

use crate::document::{inlines_plain, Block, Document, Inline, Inlines, Table};
use crate::error::ConvertError;
use crate::options::Options;
use crate::traits::Writer;
use crate::util::{lossy_winansi, winansi_byte};

pub struct PdfWriter;

pub static WRITER: PdfWriter = PdfWriter;

impl Writer for PdfWriter {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError> {
        Layout::new(options).render(document)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

impl Face {
    fn resource(self) -> &'static str {
        match self {
            Face::Regular => "F1",
            Face::Bold => "F2",
            Face::Italic => "F3",
            Face::BoldItalic => "F4",
            Face::Mono => "F5",
        }
    }

    fn base_font(self) -> &'static str {
        match self {
            Face::Regular => "Helvetica",
            Face::Bold => "Helvetica-Bold",
            Face::Italic => "Helvetica-Oblique",
            Face::BoldItalic => "Helvetica-BoldOblique",
            Face::Mono => "Courier",
        }
    }

    fn width(self, character: char) -> f32 {
        match self {
            Face::Mono => {
                if character == ' ' {
                    600.0
                } else {
                    600.0
                }
            }
            Face::Regular | Face::Italic => helvetica_width(character, false),
            Face::Bold | Face::BoldItalic => helvetica_width(character, true),
        }
    }
}

#[derive(Clone, Debug)]
struct Run {
    text: String,
    face: Face,
    size: f32,
    shade: f32,
}

#[derive(Clone, Debug)]
struct Line {
    runs: Vec<Run>,
}

struct Layout<'a> {
    options: &'a Options,
    page_width: f32,
    page_height: f32,
    margin: f32,
    body_size: f32,
    pages: Vec<Vec<u8>>,
    current: Vec<u8>,
    y: f32,
    page_number: usize,
}

impl<'a> Layout<'a> {
    fn new(options: &'a Options) -> Self {
        let (width, height) = options.pdf.page_size.points();
        let margin = options.pdf.margin_pt.clamp(24.0, 120.0);
        Self {
            options,
            page_width: width,
            page_height: height,
            margin,
            body_size: options.pdf.body_pt.clamp(7.0, 18.0),
            pages: Vec::new(),
            current: Vec::new(),
            y: height - margin,
            page_number: 1,
        }
    }

    fn content_width(&self) -> f32 {
        self.page_width - self.margin * 2.0
    }

    fn bottom(&self) -> f32 {
        self.margin + 18.0
    }

    fn render(mut self, document: &Document) -> Result<Vec<u8>, ConvertError> {
        for (index, block) in document.blocks.iter().enumerate() {
            if index > 0 {
                self.add_space(2.0);
            }
            self.block(block, 0.0);
        }
        self.finish_page();
        // A single page needs no running footer, and skipping it keeps the
        // proof free of a stray "1 / 1" when read back.
        if self.options.pdf.show_page_numbers && self.pages.len() > 1 {
            self.decorate_pages();
        }
        self.serialize(document.title.as_deref())
    }

    fn block(&mut self, block: &Block, indent: f32) {
        match block {
            Block::Heading { level, content } => {
                let size = match level {
                    1 => 21.0,
                    2 => 16.0,
                    3 => 13.5,
                    _ => self.body_size + 1.0,
                };
                self.add_space(if *level <= 2 { 14.0 } else { 9.0 });
                let runs = flatten(content, Face::Bold, size, 0.12);
                self.paragraph_with_indent(&runs, indent, 0.0, size * 1.3, 5.0);
            }
            Block::Paragraph(content) => {
                let runs = flatten(content, Face::Regular, self.body_size, 0.10);
                self.paragraph_with_indent(
                    &runs,
                    indent,
                    0.0,
                    self.body_size * 1.42,
                    5.0,
                );
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
                        "•".to_string()
                    };
                    let mut first = true;
                    for child in &item.blocks {
                        if let Block::Paragraph(content) = child {
                            let mut runs = flatten(
                                content,
                                Face::Regular,
                                self.body_size,
                                0.10,
                            );
                            if first {
                                let mut marked = vec![Run {
                                    text: marker.clone(),
                                    face: Face::Regular,
                                    size: self.body_size,
                                    shade: 0.25,
                                }];
                                // A tab-sized gap between marker and text.
                                marked.push(Run {
                                    text: "  ".to_string(),
                                    face: Face::Regular,
                                    size: self.body_size,
                                    shade: 0.10,
                                });
                                marked.append(&mut runs);
                                runs = marked;
                            }
                            self.paragraph_hanging(
                                &runs,
                                indent,
                                if first { 20.0 } else { 0.0 },
                                self.body_size * 1.42,
                                2.0,
                            );
                            first = false;
                        } else {
                            self.block(child, indent + 20.0);
                        }
                    }
                    if first {
                        self.text_line(
                            &[Run {
                                text: marker,
                                face: Face::Regular,
                                size: self.body_size,
                                shade: 0.25,
                            }],
                            indent,
                            self.body_size * 1.42,
                        );
                    }
                }
                self.add_space(4.0);
            }
            Block::Table(table) => {
                self.table(table, indent);
                self.add_space(4.0);
            }
            Block::Code { text, .. } => {
                self.code_block(text, indent);
                self.add_space(4.0);
            }
            Block::Quote(inner) => {
                self.add_space(3.0);
                let start_page = self.pages.len();
                let top = self.y;
                for child in inner {
                    self.block(child, indent + 16.0);
                }
                let bottom = self.y;
                let _ = start_page;
                if top > bottom {
                    self.rect(
                        self.margin + indent + 2.0,
                        bottom,
                        1.5,
                        top - bottom,
                        0.82,
                        0.82,
                        0.82,
                    );
                }
                self.add_space(3.0);
            }
            Block::Image(image) => {
                let label = if image.alt.is_empty() {
                    "[image omitted: untitled]".to_string()
                } else {
                    format!("[image omitted: {}]", image.alt)
                };
                let runs = vec![Run {
                    text: label,
                    face: Face::Italic,
                    size: self.body_size,
                    shade: 0.45,
                }];
                self.paragraph_with_indent(
                    &runs,
                    indent,
                    0.0,
                    self.body_size * 1.42,
                    4.0,
                );
            }
            Block::Rule => {
                self.add_space(4.0);
                self.ensure_space(2.0);
                let width = self.content_width() - indent;
                self.rect(
                    self.margin + indent,
                    self.y,
                    width,
                    0.75,
                    0.7,
                    0.7,
                    0.7,
                );
                self.y -= 8.0;
            }
        }
    }

    fn paragraph_with_indent(
        &mut self,
        runs: &[Run],
        indent: f32,
        first_indent: f32,
        leading: f32,
        after: f32,
    ) {
        let lines = wrap_runs(runs, self.content_width() - indent, self.body_size);
        for (index, line) in lines.iter().enumerate() {
            self.ensure_space(leading);
            let extra = if index == 0 { first_indent } else { 0.0 };
            self.text_line(line_runs(line), self.margin + indent + extra, leading);
        }
        self.y -= after;
    }

    fn paragraph_hanging(
        &mut self,
        runs: &[Run],
        indent: f32,
        hanging: f32,
        leading: f32,
        after: f32,
    ) {
        // The first line starts at `indent`, continuation lines at
        // `indent + hanging` — the opposite of a hanging indent, which is what
        // a list marker needs: the marker sits in the margin.
        let lines = wrap_runs(runs, self.content_width() - indent - hanging, self.body_size);
        for (index, line) in lines.iter().enumerate() {
            self.ensure_space(leading);
            let x = if index == 0 {
                self.margin + indent
            } else {
                self.margin + indent + hanging
            };
            self.text_line(line_runs(line), x, leading);
        }
        self.y -= after;
    }

    fn text_line(&mut self, runs: &[Run], x: f32, leading: f32) {
        if runs.is_empty() {
            self.y -= leading;
            return;
        }
        self.ensure_space(leading);
        self.y -= leading;
        let mut cursor = x;
        for run in runs {
            if run.text.is_empty() {
                continue;
            }
            let shade = run.shade;
            self.op(&format!("{shade:.2} g\n"));
            self.show_text(run.face, run.size, cursor, self.y, &run.text);
            cursor += measure(&run.text, run.face, run.size);
        }
        // Reset to black for subsequent drawing operations.
        self.op("0 g\n");
    }

    fn op(&mut self, operator: &str) {
        self.current.extend_from_slice(operator.as_bytes());
    }

    fn show_text(&mut self, face: Face, size: f32, x: f32, y: f32, text: &str) {
        let prefix = format!("BT /{} {:.2} Tf {:.2} {:.2} Td (", face.resource(), size, x, y);
        self.current.extend_from_slice(prefix.as_bytes());
        push_pdf_string(&mut self.current, text);
        self.current.extend_from_slice(b") Tj ET\n");
    }

    fn code_block(&mut self, text: &str, indent: f32) {
        let size = (self.body_size - 1.0).max(7.5);
        let leading = size * 1.35;
        let available = self.content_width() - indent;
        let mut lines: Vec<String> = Vec::new();
        for raw in text.lines() {
            if raw.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut line = String::new();
            for character in raw.chars() {
                let next = format!("{line}{character}");
                if measure(&next, Face::Mono, size) > available - 8.0 && !line.is_empty() {
                    lines.push(line);
                    line = character.to_string();
                } else {
                    line = next;
                }
            }
            lines.push(line);
        }
        if lines.is_empty() {
            return;
        }
        let top_padding = 6.0;
        let bottom_padding = 6.0;
        let total = lines.len() as f32 * leading + top_padding + bottom_padding;
        // Move a whole short block to the next page; long blocks flow.
        if total < self.page_height - self.margin * 2.0 {
            self.ensure_space(total);
        }
        let start_y = self.y;
        self.op(&format!(
            "0.94 0.94 0.93 rg {:.2} {:.2} {:.2} {:.2} re f 0 g\n",
            self.margin + indent,
            start_y - total + 2.0,
            available,
            total - 2.0
        ));
        self.y -= top_padding;
        for line in lines {
            self.ensure_space(leading);
            self.y -= leading;
            if !line.is_empty() {
                self.show_text(Face::Mono, size, self.margin + indent + 4.0, self.y, &line);
            }
        }
        self.y -= bottom_padding;
    }

    fn table(&mut self, table: &Table, indent: f32) {
        if table.is_empty() {
            return;
        }
        let columns = table.column_count().max(1);
        let available = self.content_width() - indent;
        let widths = column_widths(table, columns, available, self.body_size);

        let rows: Vec<(bool, Vec<Inlines>)> = {
            let mut rows = Vec::new();
            if !table.headers.is_empty() {
                rows.push((true, table.headers.clone()));
            }
            for row in &table.rows {
                rows.push((false, row.clone()));
            }
            rows
        };

        for (is_header, row) in rows {
            let cells: Vec<Vec<Line>> = (0..columns)
                .map(|index| {
                    let width = widths.get(index).copied().unwrap_or(0.0) - 8.0;
                    let content = row.get(index).cloned().unwrap_or_default();
                    let face = if is_header { Face::Bold } else { Face::Regular };
                    let runs = flatten(&content, face, self.body_size - 0.5, 0.08);
                    wrap_runs(&runs, width.max(12.0), self.body_size - 0.5)
                })
                .collect();
            let leading = (self.body_size - 0.5) * 1.35;
            let lines = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let height = lines as f32 * leading + 8.0;
            if self.y - height < self.bottom() {
                self.new_page();
            }
            self.y -= height;
            if is_header {
                self.rect(
                    self.margin + indent,
                    self.y,
                    available,
                    height,
                    0.93,
                    0.93,
                    0.92,
                );
            }
            let mut x = self.margin + indent;
            for (index, cell_lines) in cells.iter().enumerate() {
                let width = widths.get(index).copied().unwrap_or(0.0);
                for (line_index, line) in cell_lines.iter().enumerate() {
                    let baseline = self.y + height - 4.0 - (line_index as f32 + 1.0) * leading
                        + leading * 0.78;
                    let mut cursor = x + 4.0;
                    for run in line.runs.iter() {
                        if run.text.is_empty() {
                            continue;
                        }
                        self.show_text(run.face, run.size, cursor, baseline, &run.text);
                        cursor += measure(&run.text, run.face, run.size);
                    }
                }
                // Vertical rule.
                self.rect(
                    x + width,
                    self.y,
                    0.4,
                    height,
                    0.78,
                    0.78,
                    0.78,
                );
                x += width;
            }
            // Horizontal rule under the row.
            self.rect(
                self.margin + indent,
                self.y,
                available,
                0.4,
                0.78,
                0.78,
                0.78,
            );
            self.y -= leading * 0.2;
        }
    }

    fn add_space(&mut self, amount: f32) {
        self.y -= amount;
        if self.y < self.bottom() {
            self.new_page();
        }
    }

    fn ensure_space(&mut self, needed: f32) {
        if self.y - needed < self.bottom() {
            self.new_page();
        }
    }

    fn new_page(&mut self) {
        self.finish_page();
        self.y = self.page_height - self.margin;
        self.page_number += 1;
    }

    fn finish_page(&mut self) {
        if self.current.is_empty() && !self.pages.is_empty() {
            return;
        }
        self.pages.push(std::mem::take(&mut self.current));
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, red: f32, green: f32, blue: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let _ = write!(
            self.current,
            "{red:.2} {green:.2} {blue:.2} rg {x:.2} {y:.2} {width:.2} {height:.2} re f 0 g\n"
        );
    }

    fn decorate_pages(&mut self) {
        let total = self.pages.len();
        for (index, page) in self.pages.iter_mut().enumerate() {
            let label = format!("{} / {}", index + 1, total);
            let width = measure(&label, Face::Regular, 8.0);
            let prefix = format!(
                "0.45 g BT /F1 8.00 Tf {:.2} {:.2} Td (",
                (self.page_width - width) / 2.0,
                self.margin - 14.0
            );
            let mut line = Vec::with_capacity(prefix.len() + label.len() + 16);
            line.extend_from_slice(prefix.as_bytes());
            push_pdf_string(&mut line, &label);
            line.extend_from_slice(b") Tj ET 0 g\n");
            page.extend_from_slice(&line);
        }
    }

    fn serialize(&self, title: Option<&str>) -> Result<Vec<u8>, ConvertError> {
        let mut objects: Vec<Vec<u8>> = Vec::new();

        // 1: catalog, 2: pages, 3–7: fonts. Filled after page objects so the
        // references are known.
        let catalog_id = add_object(
            &mut objects,
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        );
        let pages_id = add_object(&mut objects, Vec::new());
        let font_ids: Vec<usize> = [
            Face::Regular,
            Face::Bold,
            Face::Italic,
            Face::BoldItalic,
            Face::Mono,
        ]
        .iter()
        .map(|face| {
            let body = format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                face.base_font()
            );
            add_object(&mut objects, body.into_bytes())
        })
        .collect();

        let mut page_ids = Vec::new();
        for (index, content) in self.pages.iter().enumerate() {
            let content_id = add_object(&mut objects, stream_object(content));
            let mut page = String::new();
            let _ = write!(
                page,
                "<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font << ",
                self.page_width, self.page_height
            );
            for (font_index, font_id) in font_ids.iter().enumerate() {
                let _ = write!(page, "/F{} {} 0 R ", font_index + 1, font_id);
            }
            let _ = write!(
                page,
                ">> >> /Contents {content_id} 0 R >>"
            );
            let _ = index;
            page_ids.push(add_object(&mut objects, page.into_bytes()));
        }

        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        let pages_body = format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            page_ids.len()
        );
        set_object(&mut objects, pages_id, pages_body.into_bytes());

        let info_id = {
            let mut info = Vec::new();
            info.extend_from_slice(b"<< /Producer (Verso) /Creator (Verso)");
            if let Some(title) = title {
                if !title.trim().is_empty() {
                    info.extend_from_slice(b" /Title (");
                    push_pdf_string(&mut info, title);
                    info.push(b')');
                }
            }
            info.extend_from_slice(b" >>");
            add_object(&mut objects, info)
        };

        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n");
        out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");
        let mut offsets: Vec<usize> = Vec::with_capacity(objects.len() + 1);
        offsets.push(0);
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            let _ = write!(out, "{} 0 obj\n", index + 1);
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        let _ = write!(out, "xref\n0 {}\n", objects.len() + 1);
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            let _ = write!(out, "{offset:010} 00000 n \n");
        }
        let _ = write!(
            out,
            "trailer\n<< /Size {} /Root {catalog_id} 0 R /Info {info_id} 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        );
        Ok(out)
    }
}

fn add_object(objects: &mut Vec<Vec<u8>>, body: Vec<u8>) -> usize {
    objects.push(body);
    objects.len()
}

fn set_object(objects: &mut [Vec<u8>], id: usize, body: Vec<u8>) {
    if let Some(slot) = objects.get_mut(id.saturating_sub(1)) {
        *slot = body;
    }
}

fn stream_object(content: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(content.len() + 64);
    body.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
    body.extend_from_slice(content);
    body.extend_from_slice(b"\nendstream");
    body
}

// ---------------------------------------------------------------------------
// Inline flattening and wrapping
// ---------------------------------------------------------------------------

fn flatten(inlines: &[Inline], face: Face, size: f32, shade: f32) -> Vec<Run> {
    let mut runs = Vec::new();
    flatten_into(inlines, face, size, shade, &mut runs);
    // Merge neighbours so the content stream stays compact.
    let mut merged: Vec<Run> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.face == run.face && (last.size - run.size).abs() < f32::EPSILON {
                last.text.push_str(&run.text);
                continue;
            }
        }
        merged.push(run);
    }
    merged
}

fn flatten_into(inlines: &[Inline], face: Face, size: f32, shade: f32, runs: &mut Vec<Run>) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                if !text.is_empty() {
                    runs.push(Run {
                        text: lossy_winansi(text),
                        face,
                        size,
                        shade,
                    });
                }
            }
            Inline::Strong(content) => {
                let face = match face {
                    Face::Italic => Face::BoldItalic,
                    Face::BoldItalic => Face::BoldItalic,
                    _ => Face::Bold,
                };
                flatten_into(content, face, size, shade, runs);
            }
            Inline::Emphasis(content) => {
                let face = match face {
                    Face::Bold => Face::BoldItalic,
                    Face::BoldItalic => Face::BoldItalic,
                    _ => Face::Italic,
                };
                flatten_into(content, face, size, shade, runs);
            }
            Inline::Strike(content) => flatten_into(content, face, size, shade, runs),
            Inline::Code(code) => runs.push(Run {
                text: lossy_winansi(code),
                face: Face::Mono,
                size: size - 1.0,
                shade: shade.min(0.22),
            }),
            Inline::Link { content, .. } => flatten_into(content, face, size, shade, runs),
            Inline::Break => runs.push(Run {
                text: "\n".to_string(),
                face,
                size,
                shade,
            }),
        }
    }
}

fn line_runs(line: &Line) -> &[Run] {
    &line.runs
}

fn wrap_runs(runs: &[Run], available: f32, body_size: f32) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    let mut width = 0.0f32;
    let mut pending_space = 0.0f32;

    let flush = |current: &mut Vec<Run>,
                 width: &mut f32,
                 lines: &mut Vec<Line>,
                 pending: &mut f32| {
        if current.is_empty() {
            return;
        }
        // Trim trailing space.
        if let Some(last) = current.last_mut() {
            while last.text.ends_with(' ') {
                last.text.pop();
            }
        }
        lines.push(Line {
            runs: std::mem::take(current),
        });
        *width = 0.0;
        *pending = 0.0;
    };

    for run in runs {
        // A hard break is its own line boundary.
        if run.text == "\n" {
            flush(&mut current, &mut width, &mut lines, &mut pending_space);
            continue;
        }
        let mut buffer = String::new();
        for character in run.text.chars() {
            if character.is_whitespace() {
                if !buffer.is_empty() {
                    let word = Run {
                        text: std::mem::take(&mut buffer),
                        ..run.clone()
                    };
                    place_word(
                        word,
                        &mut current,
                        &mut width,
                        &mut pending_space,
                        &mut lines,
                        available,
                    );
                }
                pending_space = measure(" ", run.face, run.size);
            } else {
                buffer.push(character);
            }
        }
        if !buffer.is_empty() {
            let word = Run {
                text: buffer,
                ..run.clone()
            };
            place_word(
                word,
                &mut current,
                &mut width,
                &mut pending_space,
                &mut lines,
                available,
            );
        }
    }
    if !current.is_empty() {
        flush(&mut current, &mut width, &mut lines, &mut pending_space);
    }
    if lines.is_empty() {
        lines.push(Line {
            runs: Vec::new(),
        });
    }
    // If a trailing space widened a line beyond the measure, it was trimmed.
    let _ = body_size;
    lines
}

fn place_word(
    word: Run,
    current: &mut Vec<Run>,
    width: &mut f32,
    pending_space: &mut f32,
    lines: &mut Vec<Line>,
    available: f32,
) {
    let word_width = measure(&word.text, word.face, word.size);
    let space = if current.is_empty() { 0.0 } else { *pending_space };
    if *width + space + word_width <= available || current.is_empty() {
        if !current.is_empty() && *pending_space > 0.0 {
            current.push(Run {
                text: " ".to_string(),
                face: word.face,
                size: word.size,
                shade: word.shade,
            });
            *width += *pending_space;
        }
        *width += word_width;
        *pending_space = 0.0;
        current.push(word);
    } else {
        // Start a new line.
        if let Some(last) = current.last_mut() {
            while last.text.ends_with(' ') {
                last.text.pop();
            }
        }
        lines.push(Line {
            runs: std::mem::take(current),
        });
        *width = word_width;
        *pending_space = 0.0;
        current.push(word);
    }
}

fn measure(text: &str, face: Face, size: f32) -> f32 {
    let units: f32 = text
        .chars()
        .map(|character| {
            let mapped = if crate::util::winansi_byte(character).is_some() {
                character
            } else {
                '?'
            };
            face.width(mapped)
        })
        .sum();
    units * size / 1000.0
}

fn column_widths(table: &Table, columns: usize, available: f32, body_size: f32) -> Vec<f32> {
    let mut natural = vec![0.0f32; columns];
    let mut consider = |index: usize, content: &Inlines| {
        let text = inlines_plain(content);
        let width = measure(&text, Face::Regular, body_size - 0.5) + 10.0;
        if let Some(slot) = natural.get_mut(index) {
            *slot = slot.max(width);
        }
    };
    for (index, cell) in table.headers.iter().enumerate() {
        consider(index, cell);
    }
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            consider(index, cell);
        }
    }
    let min = 42.0f32;
    for width in &mut natural {
        *width = width.max(min);
    }
    let total: f32 = natural.iter().sum();
    if total <= available {
        // Distribute leftover space in proportion to natural width.
        natural
            .iter()
            .map(|width| width + (available - total) * (width / total))
            .collect()
    } else {
        let factor = available / total;
        natural.iter().map(|width| width * factor).collect()
    }
}

fn helvetica_width(character: char, bold: bool) -> f32 {
    let table: &[u16] = if bold { &BOLD_WIDTHS } else { &REGULAR_WIDTHS };
    let code = character as u32;
    if (32..=126).contains(&code) {
        f32::from(table[(code - 32) as usize])
    } else {
        match character {
            '\u{2018}' | '\u{2019}' => 222.0,
            '\u{201C}' | '\u{201D}' => if bold { 500.0 } else { 444.0 },
            '\u{2013}' => 556.0,
            '\u{2014}' => 1000.0,
            '\u{2022}' => 350.0,
            '\u{2026}' => 1000.0,
            _ => 556.0,
        }
    }
}

/// Helvetica AFM advance widths for ASCII 32–126.
const REGULAR_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Helvetica-Bold AFM advance widths for ASCII 32–126.
const BOLD_WIDTHS: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

use crate::format::Format;
use crate::traits::Reader;

pub struct PdfReader;

pub static READER: PdfReader = PdfReader;

impl Reader for PdfReader {
    fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError> {
        options.check_size(input)?;
        if !input.starts_with(b"%PDF-") {
            // Some producers put junk before the header.
            if !input
                .windows(b"%PDF-".len())
                .take(1024)
                .any(|window| window == b"%PDF-")
            {
                return Err(ConvertError::NotThisFormat {
                    expected: Format::Pdf,
                });
            }
        }

        let pages = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pdf_extract::extract_text_from_mem_by_pages(input)
        }))
        .map_err(|_| ConvertError::PdfUnreadable)?
        .map_err(|error| {
            let message = error.to_string().to_ascii_lowercase();
            if message.contains("encrypt") || message.contains("password") {
                ConvertError::PdfUnreadable
            } else {
                ConvertError::parse(Format::Pdf, error)
            }
        })?;

        let mut blocks = Vec::new();
        let mut saw_text = false;
        for page in &pages {
            let page = page.replace('\r', "\n");
            let page = page.replace('\u{0C}', "\n\n");
            for paragraph in page.split("\n\n") {
                let text = crate::util::collapse_whitespace(paragraph);
                if text.is_empty() {
                    continue;
                }
                saw_text = true;
                blocks.push(Block::Paragraph(vec![Inline::Text(text)]));
            }
        }

        if !saw_text {
            return Err(ConvertError::ScannedPdf);
        }
        let mut document = Document::new(blocks);
        document.notices.push(
            "PDF text is positional: headings, lists and tables were recovered as plain paragraphs."
                .to_string(),
        );
        Ok(document)
    }
}

fn push_pdf_string(out: &mut Vec<u8>, text: &str) {
    for character in text.chars() {
        match character {
            '(' => out.extend_from_slice(b"\\("),
            ')' => out.extend_from_slice(b"\\)"),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\r' => {}
            '\n' => out.push(b' '),
            _ => out.push(winansi_byte(character).unwrap_or(b'?')),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::PageSize;

    fn sample_document() -> Document {
        Document::new(vec![
            Block::Heading {
                level: 1,
                content: vec![Inline::Text("Quarterly report".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("A paragraph with ".into()),
                Inline::Strong(vec![Inline::Text("bold".into())]),
                Inline::Text(" text and a ".into()),
                Inline::Link {
                    content: vec![Inline::Text("link".into())],
                    href: "https://example.com".into(),
                },
                Inline::Text(".".into()),
            ]),
            Block::List {
                ordered: false,
                start: 1,
                items: vec![
                    crate::document::ListItem::paragraph("First item"),
                    crate::document::ListItem::paragraph("Second item"),
                ],
            },
            Block::Table(Table {
                headers: vec![
                    vec![Inline::Text("Name".into())],
                    vec![Inline::Text("Value".into())],
                ],
                rows: vec![vec![
                    vec![Inline::Text("alpha".into())],
                    vec![Inline::Text("one".into())],
                ]],
                align: Vec::new(),
            }),
        ])
    }

    #[test]
    fn writes_a_readable_pdf_header_and_trailer() {
        let options = Options::default();
        let bytes = WRITER.write(&sample_document(), &options).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/BaseFont /Helvetica"));
        assert!(text.contains("startxref"));
    }

    #[test]
    fn honours_page_size() {
        let options = Options::default().with_page_size(PageSize::Letter);
        let bytes = WRITER.write(&sample_document(), &options).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("612.00 792.00"));
    }

    #[test]
    fn wraps_long_lines() {
        let long = "word ".repeat(1200);
        let document = Document::new(vec![Block::Paragraph(vec![Inline::Text(long)])]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        // The content stream is compressed nowhere, so page count is visible.
        assert!(text.matches("/Type /Page ").count() >= 2);
    }

    #[test]
    fn empty_document_still_has_one_page() {
        let bytes = WRITER.write(&Document::default(), &Options::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Type /Page ").count(), 1);
    }

    #[test]
    fn a_single_page_has_no_page_footer() {
        let document =
            Document::new(vec![Block::Paragraph(vec![Inline::Text("Just one page.".into())])]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("(1 / 1)"), "no running footer on one page");
    }

    #[test]
    fn longer_documents_number_their_pages() {
        let long = "word ".repeat(2400);
        let document = Document::new(vec![Block::Paragraph(vec![Inline::Text(long)])]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("(1 / "), "the first page is numbered");
    }

    #[test]
    fn writes_winansi_bytes_not_utf8() {
        let document =
            Document::new(vec![Block::Paragraph(vec![Inline::Text("café".into())])]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        assert!(bytes.windows(4).any(|window| window == b"caf\xE9"));
        assert!(!bytes.windows(5).any(|window| window == b"caf\xC3\xA9"));
    }

    #[test]
    fn reads_its_own_text_back() {
        let document = Document::new(vec![
            Block::Heading {
                level: 1,
                content: vec![Inline::Text("A readable title".into())],
            },
            Block::Paragraph(vec![Inline::Text(
                "The quick brown fox jumps over the lazy dog.".into(),
            )]),
        ]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let back = READER.read(&bytes, &Options::default());
        let back = back.unwrap_or_else(|error| panic!("{error}"));
        let text = back.text_only();
        assert!(text.contains("A readable title"), "{text}");
        assert!(text.contains("quick brown fox"), "{text}");
    }

    #[test]
    fn a_pdf_without_text_is_a_scan() {
        let options = Options {
            pdf: crate::options::PdfOptions {
                show_page_numbers: false,
                ..Options::default().pdf
            },
            ..Options::default()
        };
        let bytes = WRITER.write(&Document::default(), &options).unwrap();
        let error = READER.read(&bytes, &Options::default()).unwrap_err();
        assert_eq!(error.code(), "scanned-pdf");
    }

    #[test]
    fn rejects_non_pdf_bytes() {
        let error = READER.read(b"hello", &Options::default()).unwrap_err();
        assert_eq!(error.code(), "wrong-format");
    }
}
