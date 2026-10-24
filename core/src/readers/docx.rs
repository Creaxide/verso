//! DOCX reading and writing.
//!
//! Reading uses the OOXML part directly (`word/document.xml`) with `roxmltree`.
//! Writing emits WordprocessingML and packages it with `zip`. Both directions
//! understand headings, inline emphasis, links, lists, tables and (on read)
//! embedded images.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write as _};

use roxmltree::Node;

use crate::document::{
    inlines_plain, Align, Block, Document, ImageRef, Inline, Inlines, ListItem, Table,
};
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::traits::{Reader, Writer};
use crate::util::escape_xml;

pub struct DocxReader;
pub struct DocxWriter;

pub static READER: DocxReader = DocxReader;
pub static WRITER: DocxWriter = DocxWriter;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

impl Reader for DocxReader {
    fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError> {
        options.check_size(input)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(input))
            .map_err(|error| ConvertError::parse(Format::Docx, error))?;

        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        if !names.iter().any(|name| name == "word/document.xml") {
            return Err(ConvertError::NotThisFormat {
                expected: Format::Docx,
            });
        }

        let document_xml = read_entry(&mut archive, "word/document.xml")
            .ok_or_else(|| ConvertError::parse(Format::Docx, "word/document.xml is missing"))?;
        let styles_xml = read_entry(&mut archive, "word/styles.xml").unwrap_or_default();
        let numbering_xml = read_entry(&mut archive, "word/numbering.xml").unwrap_or_default();
        let rels_xml = read_entry(&mut archive, "word/_rels/document.xml.rels").unwrap_or_default();
        let core_xml = read_entry(&mut archive, "docProps/core.xml").unwrap_or_default();

        let mut media = HashMap::new();
        for name in names.iter() {
            if name.starts_with("word/media/") {
                if let Some(bytes) = read_entry(&mut archive, name) {
                    let mime = mime_for(name).to_string();
                    media.insert(name.clone(), (mime, bytes));
                }
            }
        }

        let document_text = String::from_utf8_lossy(&document_xml);
        let document = roxmltree::Document::parse(&document_text)
            .map_err(|error| ConvertError::parse(Format::Docx, error))?;
        let body = document
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "body")
            .ok_or_else(|| ConvertError::parse(Format::Docx, "the document body is missing"))?;

        let mut context = Context {
            rels: parse_rels(&String::from_utf8_lossy(&rels_xml)),
            media,
            styles: parse_styles(&String::from_utf8_lossy(&styles_xml)),
            numbering: parse_numbering(&String::from_utf8_lossy(&numbering_xml)),
            notices: Vec::new(),
            missing_images: false,
            flattened_lists: false,
            merged_cells: false,
        };

        let mut blocks = Vec::new();
        parse_container(body, &mut blocks, &mut context);

        let title = parse_title(&String::from_utf8_lossy(&core_xml));
        let mut document = Document {
            title,
            blocks,
            notices: Vec::new(),
        };
        if context.missing_images {
            document.notices.push(
                "Some images referenced by this document were missing and were skipped.".to_string(),
            );
        }
        if context.flattened_lists {
            document
                .notices
                .push("Nested list levels were flattened to one level.".to_string());
        }
        if context.merged_cells {
            document
                .notices
                .push("Merged table cells were expanded, which can shift columns.".to_string());
        }
        Ok(document)
    }
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut file = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

struct Context {
    rels: HashMap<String, String>,
    media: HashMap<String, (String, Vec<u8>)>,
    styles: HashMap<String, StyleInfo>,
    numbering: HashMap<String, bool>,
    notices: Vec<String>,
    missing_images: bool,
    flattened_lists: bool,
    merged_cells: bool,
}

#[derive(Clone, Debug, Default)]
struct StyleInfo {
    name: String,
    outline: Option<u8>,
}

struct ParsedParagraph {
    inlines: Inlines,
    style: Option<String>,
    num_id: Option<String>,
    level: Option<u8>,
    images: Vec<ImageRef>,
}

fn parse_container(container: Node<'_, '_>, blocks: &mut Vec<Block>, ctx: &mut Context) {
    let mut pending: Option<PendingList> = None;

    for child in container.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "p" => {
                let paragraph = parse_paragraph(child, ctx);
                if let Some(num_id) = &paragraph.num_id {
                    let ordered = ctx.numbering.get(num_id).copied().unwrap_or(false);
                    let start_new = pending
                        .as_ref()
                        .map(|list| list.ordered != ordered)
                        .unwrap_or(true);
                    if start_new {
                        flush_list(&mut pending, blocks);
                        pending = Some(PendingList {
                            ordered,
                            items: Vec::new(),
                        });
                    }
                    let mut item_blocks = Vec::new();
                    if !plain(&paragraph.inlines).trim().is_empty() {
                        item_blocks.push(Block::Paragraph(paragraph.inlines));
                    }
                    for image in paragraph.images {
                        item_blocks.push(Block::Image(image));
                    }
                    if !item_blocks.is_empty() {
                        if let Some(list) = pending.as_mut() {
                            list.items.push(ListItem { blocks: item_blocks });
                        }
                    }
                } else {
                    flush_list(&mut pending, blocks);
                    emit_paragraph(paragraph, false, blocks, ctx);
                }
            }
            "tbl" => {
                flush_list(&mut pending, blocks);
                if let Some(table) = parse_table(child, ctx) {
                    blocks.push(Block::Table(table));
                }
            }
            "sdt" => {
                flush_list(&mut pending, blocks);
                if let Some(content) = child
                    .children()
                    .find(|node| node.is_element() && node.tag_name().name() == "sdtContent")
                {
                    parse_container(content, blocks, ctx);
                }
            }
            "sectPr" | "bookmarkStart" | "bookmarkEnd" => {}
            _ => {}
        }
    }
    flush_list(&mut pending, blocks);
}

fn flush_list(pending: &mut Option<PendingList>, blocks: &mut Vec<Block>) {
    if let Some(list) = pending.take() {
        if !list.items.is_empty() {
            blocks.push(Block::List {
                ordered: list.ordered,
                start: 1,
                items: list.items,
            });
        }
    }
}

struct PendingList {
    ordered: bool,
    items: Vec<ListItem>,
}

fn emit_paragraph(
    paragraph: ParsedParagraph,
    in_cell: bool,
    blocks: &mut Vec<Block>,
    ctx: &mut Context,
) {
    let style = paragraph
        .style
        .as_deref()
        .map(|style| style.to_ascii_lowercase())
        .unwrap_or_default();
    let style_info = paragraph
        .style
        .as_ref()
        .and_then(|id| ctx.styles.get(id))
        .cloned()
        .unwrap_or_default();
    let style_name = style_info.name.to_ascii_lowercase();

    let heading = heading_level(&style, &style_name, paragraph.level.or(style_info.outline));
    if !in_cell {
        if let Some(level) = heading {
            blocks.push(Block::Heading {
                level,
                content: paragraph.inlines,
            });
            for image in paragraph.images {
                blocks.push(Block::Image(image));
            }
            return;
        }
        if style.contains("quote") || style_name.contains("quote") {
            blocks.push(Block::Quote(vec![Block::Paragraph(paragraph.inlines)]));
            for image in paragraph.images {
                blocks.push(Block::Image(image));
            }
            return;
        }
        if style.contains("code") || style.contains("pre") || style_name.contains("code") {
            let text = inlines_plain(&paragraph.inlines);
            blocks.push(Block::Code {
                language: None,
                text: text.trim_end_matches('\n').to_string(),
            });
            for image in paragraph.images {
                blocks.push(Block::Image(image));
            }
            return;
        }
    }

    if !plain(&paragraph.inlines).trim().is_empty() {
        blocks.push(Block::Paragraph(paragraph.inlines));
    }
    for image in paragraph.images {
        if in_cell {
            ctx.notices
                .push("Images inside table cells were skipped.".to_string());
        } else {
            blocks.push(Block::Image(image));
        }
    }
}

fn heading_level(style: &str, style_name: &str, outline: Option<u8>) -> Option<u8> {
    for candidate in [style, style_name] {
        let compact = candidate.replace([' ', '-', '_'], "");
        if compact == "title" {
            return Some(1);
        }
        if compact == "subtitle" {
            return Some(2);
        }
        if let Some(rest) = compact.strip_prefix("heading") {
            if let Ok(level) = rest.parse::<u8>() {
                if (1..=6).contains(&level) {
                    return Some(level);
                }
            }
        }
    }
    outline.map(|level| level.clamp(0, 5) + 1)
}

fn parse_paragraph(node: Node<'_, '_>, ctx: &mut Context) -> ParsedParagraph {
    let mut style = None;
    let mut num_id = None;
    let mut level = None;
    if let Some(properties) = child(node, "pPr") {
        if let Some(value) = child(properties, "pStyle").and_then(|style| attr(style, "val")) {
            style = Some(value.to_string());
        }
        if let Some(value) = child(properties, "outlineLvl").and_then(|level| attr(level, "val")) {
            level = value.parse::<u8>().ok();
        }
        if let Some(numbering) = child(properties, "numPr") {
            if let Some(value) = child(numbering, "numId").and_then(|id| attr(id, "val")) {
                if value != "0" {
                    num_id = Some(value.to_string());
                }
            }
            if let Some(value) = child(numbering, "ilvl").and_then(|id| attr(id, "val")) {
                if value.parse::<u8>().unwrap_or(0) > 0 {
                    ctx.flattened_lists = true;
                }
            }
        }
        if let Some(value) = child(properties, "jc").and_then(|align| attr(align, "val")) {
            let _ = value;
        }
    }

    let mut inlines = Vec::new();
    let mut images = Vec::new();
    parse_inlines(node, ctx, &mut inlines, &mut images, false, false, false);

    ParsedParagraph {
        inlines,
        style,
        num_id,
        level,
        images,
    }
}

fn parse_inlines(
    node: Node<'_, '_>,
    ctx: &mut Context,
    out: &mut Inlines,
    images: &mut Vec<ImageRef>,
    bold: bool,
    italic: bool,
    strike: bool,
) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "r" => parse_run(child, ctx, out, images, bold, italic, strike),
            "hyperlink" => {
                let mut content = Vec::new();
                parse_inlines(
                    child, ctx, &mut content, images, bold, italic, strike,
                );
                if content.is_empty() {
                    continue;
                }
                let href = attr(child, "id")
                    .and_then(|id| ctx.rels.get(id))
                    .cloned()
                    .unwrap_or_default();
                if href.is_empty() {
                    out.extend(content);
                } else {
                    out.push(Inline::Link {
                        content,
                        href,
                    });
                }
            }
            "ins" | "smartTag" | "sdt" | "sdtContent" | "fldSimple" => {
                parse_inlines(child, ctx, out, images, bold, italic, strike);
            }
            "del" | "moveFrom" => {}
            _ => {}
        }
    }
}

fn parse_run(
    node: Node<'_, '_>,
    ctx: &mut Context,
    out: &mut Inlines,
    images: &mut Vec<ImageRef>,
    bold: bool,
    italic: bool,
    strike: bool,
) {
    let mut bold = bold;
    let mut italic = italic;
    let mut strike = strike;
    let mut code = false;
    if let Some(properties) = child(node, "rPr") {
        bold |= any_child(properties, "b");
        italic |= any_child(properties, "i");
        strike |= any_child(properties, "strike") || any_child(properties, "dstrike");
        if let Some(value) = child(properties, "rStyle").and_then(|style| attr(style, "val")) {
            let value = value.to_ascii_lowercase();
            code = value.contains("code") || value.contains("pre");
        }
    }

    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "t" => {
                let text = child.text().unwrap_or("");
                if !text.is_empty() {
                    push_inline(out, Inline::Text(text.to_string()), bold, italic, strike, code);
                }
            }
            "tab" => push_inline(out, Inline::Text("\t".to_string()), bold, italic, strike, code),
            "br" => push_inline(out, Inline::Break, bold, italic, strike, code),
            "cr" => push_inline(out, Inline::Break, bold, italic, strike, code),
            "noBreakHyphen" => {
                push_inline(out, Inline::Text("-".to_string()), bold, italic, strike, code)
            }
            "softHyphen" => {}
            "drawing" | "pict" => {
                if let Some(image) = parse_image(child, ctx) {
                    images.push(image);
                }
            }
            "rPr" | "fldChar" | "instrText" | "lastRenderedPageBreak" => {}
            _ => {}
        }
    }
}

fn push_inline(
    out: &mut Inlines,
    inline: Inline,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
) {
    if code {
        if let Inline::Text(text) = &inline {
            out.push(Inline::Code(text.clone()));
            return;
        }
    }
    let wrapped = wrap(match inline {
        Inline::Text(text) => Inline::Text(text),
        Inline::Break => Inline::Break,
        other => other,
    }, bold, italic, strike, code);
    out.push(wrapped);
}

fn wrap(inline: Inline, bold: bool, italic: bool, strike: bool, _code: bool) -> Inline {
    let mut current = inline;
    if strike {
        current = Inline::Strike(vec![current]);
    }
    if italic {
        current = Inline::Emphasis(vec![current]);
    }
    if bold {
        current = Inline::Strong(vec![current]);
    }
    current
}

fn parse_image(node: Node<'_, '_>, ctx: &mut Context) -> Option<ImageRef> {
    let blip = node
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "blip")?;
    let id = attr(blip, "embed")?;
    let target = ctx.rels.get(id)?;
    let path = if target.starts_with("word/") {
        target.clone()
    } else if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("word/{target}")
    };
    let Some((mime, data)) = ctx.media.get(&path).cloned() else {
        ctx.missing_images = true;
        return None;
    };
    let alt = node
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "docPr")
        .and_then(|node| attr(node, "descr").or_else(|| attr(node, "name")))
        .unwrap_or("")
        .to_string();
    Some(ImageRef {
        alt,
        mime,
        data: Some(data),
        source: None,
    })
}

fn parse_table(node: Node<'_, '_>, ctx: &mut Context) -> Option<Table> {
    let mut all_rows: Vec<(bool, Vec<Inlines>)> = Vec::new();
    for row in node
        .children()
        .filter(Node::is_element)
        .filter(|child| child.tag_name().name() == "tr")
    {
        let header = child(row, "trPr")
            .map(|properties| any_child(properties, "tblHeader"))
            .unwrap_or(false);
        let mut cells = Vec::new();
        for cell in row
            .children()
            .filter(Node::is_element)
            .filter(|child| child.tag_name().name() == "tc")
        {
            if let Some(properties) = child(cell, "tcPr") {
                if let Some(span) = child(properties, "gridSpan").and_then(|span| attr(span, "val")) {
                    if span.parse::<u32>().unwrap_or(1) > 1 {
                        ctx.merged_cells = true;
                    }
                }
                if any_child(properties, "vMerge") {
                    ctx.merged_cells = true;
                }
            }
            let mut paragraphs: Vec<Inlines> = Vec::new();
            collect_cell_paragraphs(cell, ctx, &mut paragraphs);
            let mut inlines = Vec::new();
            for (index, paragraph) in paragraphs.into_iter().enumerate() {
                if index > 0 {
                    inlines.push(Inline::Break);
                }
                inlines.extend(paragraph);
            }
            cells.push(inlines);
        }
        if !cells.is_empty() {
            all_rows.push((header, cells));
        }
    }
    if all_rows.is_empty() {
        return None;
    }
    let first_is_header = all_rows.first().map(|(header, _)| *header).unwrap_or(false);
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    for (index, (_, cells)) in all_rows.into_iter().enumerate() {
        if index == 0 && first_is_header {
            headers = cells;
        } else {
            rows.push(cells);
        }
    }
    let columns = rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(headers.len()))
        .max()
        .unwrap_or(0);
    Some(Table {
        headers,
        rows,
        align: vec![Align::Left; columns],
    })
}

fn collect_cell_paragraphs(node: Node<'_, '_>, ctx: &mut Context, out: &mut Vec<Inlines>) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "p" => {
                let paragraph = parse_paragraph(child, ctx);
                if !plain(&paragraph.inlines).trim().is_empty() {
                    out.push(paragraph.inlines);
                }
            }
            "tbl" => {
                // Nested tables are flattened into their cell text.
                collect_cell_paragraphs(child, ctx, out);
            }
            "sdt" | "sdtContent" => collect_cell_paragraphs(child, ctx, out),
            _ => {}
        }
    }
}

fn plain(inlines: &[Inline]) -> String {
    inlines.iter().map(Inline::plain).collect()
}

fn mime_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".emf") {
        "image/emf"
    } else if lower.ends_with(".wmf") {
        "image/wmf"
    } else {
        "application/octet-stream"
    }
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

fn any_child(node: Node<'_, '_>, name: &str) -> bool {
    child(node, name).is_some()
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}

fn parse_rels(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return map;
    };
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "Relationship")
    {
        let Some(id) = attr(node, "Id") else { continue };
        let Some(target) = attr(node, "Target") else {
            continue;
        };
        map.insert(id.to_string(), target.to_string());
    }
    map
}

fn parse_styles(xml: &str) -> HashMap<String, StyleInfo> {
    let mut map = HashMap::new();
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return map;
    };
    for style in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "style")
    {
        let Some(id) = attr(style, "styleId") else {
            continue;
        };
        let name = child(style, "name")
            .and_then(|node| attr(node, "val"))
            .unwrap_or("")
            .to_string();
        let outline = child(style, "pPr")
            .and_then(|properties| child(properties, "outlineLvl"))
            .and_then(|level| attr(level, "val"))
            .and_then(|value| value.parse::<u8>().ok());
        map.insert(id.to_string(), StyleInfo { name, outline });
    }
    map
}

fn parse_numbering(xml: &str) -> HashMap<String, bool> {
    let mut formats: HashMap<String, bool> = HashMap::new();
    let mut result = HashMap::new();
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return result;
    };
    for abstract_num in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "abstractNum")
    {
        let Some(id) = attr(abstract_num, "abstractNumId") else {
            continue;
        };
        let level = abstract_num
            .children()
            .filter(Node::is_element)
            .find(|child| child.tag_name().name() == "lvl");
        let format = level
            .and_then(|level| child(level, "numFmt"))
            .and_then(|format| attr(format, "val"))
            .unwrap_or("bullet");
        formats.insert(id.to_string(), format != "bullet");
    }
    for num in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "num")
    {
        let Some(id) = attr(num, "numId") else { continue };
        let abstract_id = child(num, "abstractNumId")
            .and_then(|node| attr(node, "val"))
            .unwrap_or("");
        if let Some(ordered) = formats.get(abstract_id) {
            result.insert(id.to_string(), *ordered);
        }
    }
    result
}

fn parse_title(xml: &str) -> Option<String> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let node = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "title")?;
    let text = node.text().unwrap_or("").trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

struct DocxBuilder<'a> {
    body: String,
    rels: String,
    next_rel: usize,
    options: &'a Options,
}

impl<'a> DocxBuilder<'a> {
    fn new(options: &'a Options) -> Self {
        Self {
            body: String::new(),
            rels: String::new(),
            next_rel: 1,
            options,
        }
    }

    fn relation(&mut self, target: &str) -> String {
        let id = format!("rId{}", self.next_rel);
        self.next_rel += 1;
        let _ = write!(
            self.rels,
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
            escape_xml(target)
        );
        id
    }

    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.block(block, 0);
        }
    }

    fn block(&mut self, block: &Block, list_level: usize) {
        match block {
            Block::Heading { level, content } => {
                let style = format!("Heading{}", (*level).clamp(1, 6));
                self.paragraph(content, Some(&style), None, 0);
            }
            Block::Paragraph(content) => {
                self.paragraph(content, None, None, list_level);
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let _ = start;
                let num_id = if *ordered { 2 } else { 1 };
                for item in items {
                    let mut first = true;
                    for child in &item.blocks {
                        match child {
                            Block::Paragraph(content) => {
                                if first {
                                    self.paragraph(content, Some("ListParagraph"), Some(num_id), list_level);
                                    first = false;
                                } else {
                                    self.paragraph(content, Some("ListParagraph"), None, list_level + 1);
                                }
                            }
                            Block::List {
                                ordered,
                                start,
                                items,
                            } => {
                                let nested = Block::List {
                                    ordered: *ordered,
                                    start: *start,
                                    items: items.clone(),
                                };
                                self.block(&nested, list_level + 1);
                            }
                            other => self.block(other, list_level + 1),
                        }
                    }
                }
            }
            Block::Table(table) => self.table(table),
            Block::Code { text, .. } => self.code(text),
            Block::Quote(inner) => {
                for child in inner {
                    match child {
                        Block::Paragraph(content) => {
                            self.paragraph(content, Some("Quote"), None, 0)
                        }
                        other => {
                            if let Block::Paragraph(content) = other {
                                self.paragraph(content, Some("Quote"), None, 0);
                            } else {
                                self.block(other, 0);
                            }
                        }
                    }
                }
            }
            Block::Image(image) => {
                let label = format!(
                    "[image omitted: {}]",
                    if image.alt.is_empty() {
                        "untitled"
                    } else {
                        &image.alt
                    }
                );
                self.paragraph(
                    &[Inline::Emphasis(vec![Inline::Text(label)])],
                    Some("Quote"),
                    None,
                    0,
                );
            }
            Block::Rule => {
                self.body.push_str(
                    r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="B7B7B0"/></w:pBdr></w:pPr></w:p>"#,
                );
            }
        }
    }

    fn paragraph(
        &mut self,
        content: &[Inline],
        style: Option<&str>,
        num_id: Option<u32>,
        list_level: usize,
    ) {
        self.body.push_str("<w:p><w:pPr>");
        if let Some(style) = style {
            let _ = write!(
                self.body,
                r#"<w:pStyle w:val="{}"/>"#,
                escape_xml(style)
            );
        }
        if let Some(num_id) = num_id {
            let _ = write!(
                self.body,
                r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{}"/></w:numPr>"#,
                list_level.min(8),
                num_id
            );
        }
        self.body.push_str("</w:pPr>");
        self.inlines(content, false, false, false);
        self.body.push_str("</w:p>");
    }

    fn inlines(&mut self, inlines: &[Inline], bold: bool, italic: bool, strike: bool) {
        for inline in inlines {
            match inline {
                Inline::Text(text) => self.run(text, bold, italic, strike, false),
                Inline::Break => self.body.push_str("<w:r><w:br/></w:r>"),
                Inline::Strong(content) => self.inlines(content, true, italic, strike),
                Inline::Emphasis(content) => self.inlines(content, bold, true, strike),
                Inline::Strike(content) => self.inlines(content, bold, italic, true),
                Inline::Code(code) => self.run(code, bold, italic, strike, true),
                Inline::Link { content, href } => {
                    if href.is_empty() {
                        self.inlines(content, bold, italic, strike);
                    } else {
                        let id = self.relation(href);
                        let _ = write!(self.body, r#"<w:hyperlink r:id="{id}" w:history="1">"#);
                        self.inlines(content, bold, italic, strike);
                        self.body.push_str("</w:hyperlink>");
                    }
                }
            }
        }
    }

    fn run(&mut self, text: &str, bold: bool, italic: bool, strike: bool, code: bool) {
        self.body.push_str("<w:r><w:rPr>");
        if code {
            self.body.push_str(
                r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:sz w:val="19"/><w:shd w:val="clear" w:color="auto" w:fill="F1F0EA"/>"#,
            );
        }
        if bold {
            self.body.push_str("<w:b/>");
        }
        if italic {
            self.body.push_str("<w:i/>");
        }
        if strike {
            self.body.push_str("<w:strike/>");
        }
        self.body.push_str("</w:rPr>");
        let _ = write!(
            self.body,
            r#"<w:t xml:space="preserve">{}</w:t></w:r>"#,
            escape_xml(text)
        );
    }

    fn code(&mut self, text: &str) {
        self.body.push_str(r#"<w:p><w:pPr><w:pStyle w:val="Code"/></w:pPr>"#);
        for (index, line) in text.lines().enumerate() {
            if index > 0 {
                self.body.push_str("<w:r><w:br/></w:r>");
            }
            if !line.is_empty() {
                self.run(line, false, false, false, true);
            }
        }
        self.body.push_str("</w:p>");
    }

    fn table(&mut self, table: &Table) {
        if table.is_empty() {
            return;
        }
        let columns = table.column_count().max(1);
        let (page_width, _) = self.options.pdf.page_size.points();
        let content_twips = ((page_width - self.options.pdf.margin_pt * 2.0) * 20.0) as u32;
        let width = content_twips / columns as u32;

        self.body.push_str(r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:left w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:right w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/></w:tblBorders></w:tblPr><w:tblGrid>"#);
        for _ in 0..columns {
            let _ = write!(self.body, r#"<w:gridCol w:w="{width}"/>"#);
        }
        self.body.push_str("</w:tblGrid>");

        let mut rows: Vec<(bool, Vec<Inlines>)> = Vec::new();
        if !table.headers.is_empty() {
            rows.push((true, table.headers.clone()));
        }
        for row in &table.rows {
            rows.push((false, row.clone()));
        }
        for (is_header, row) in rows {
            self.body.push_str("<w:tr>");
            if is_header {
                self.body.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for index in 0..columns {
                self.body.push_str("<w:tc><w:tcPr>");
                let _ = write!(self.body, r#"<w:tcW w:w="{width}" w:type="dxa"/>"#);
                if is_header {
                    self.body.push_str(r#"<w:shd w:val="clear" w:color="auto" w:fill="F0EFE9"/>"#);
                }
                self.body.push_str("</w:tcPr><w:p>");
                let cell = row.get(index).cloned().unwrap_or_default();
                if is_header {
                    let bold: Inlines = cell
                        .into_iter()
                        .map(|inline| Inline::Strong(vec![inline]))
                        .collect();
                    self.inlines(&bold, true, false, false);
                } else {
                    self.inlines(&cell, false, false, false);
                }
                self.body.push_str("</w:p></w:tc>");
            }
            self.body.push_str("</w:tr>");
        }
        self.body.push_str("</w:tbl><w:p/>");
    }
}

impl Writer for DocxWriter {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError> {
        let mut builder = DocxBuilder::new(options);
        builder.blocks(&document.blocks);

        let (page_width, page_height) = options.pdf.page_size.points();
        let margin_twips = (options.pdf.margin_pt * 20.0) as u32;
        let section = format!(
            r#"<w:sectPr><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{margin}" w:right="{margin}" w:bottom="{margin}" w:left="{margin}" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>"#,
            (page_width * 20.0) as u32,
            (page_height * 20.0) as u32,
            margin = margin_twips
        );

        let document_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body>{}{section}</w:body></w:document>"#,
            builder.body
        );
        let document_rels = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
            builder.rels
        );
        let title = document.title.clone().unwrap_or_default();
        let core = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Verso</dc:creator></cp:coreProperties>"#,
            escape_xml(&title)
        );

        write_package(&[
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("docProps/core.xml", &core),
            ("word/document.xml", &document_xml),
            ("word/styles.xml", STYLES),
            ("word/numbering.xml", NUMBERING),
            ("word/_rels/document.xml.rels", &document_rels),
        ])
    }
}

fn write_package(entries: &[(&str, &str)]) -> Result<Vec<u8>, ConvertError> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, content) in entries {
        writer
            .start_file(*name, options)
            .map_err(|error| ConvertError::encode(Format::Docx, error))?;
        writer
            .write_all(content.as_bytes())
            .map_err(|error| ConvertError::encode(Format::Docx, error))?;
    }
    let cursor = writer
        .finish()
        .map_err(|error| ConvertError::encode(Format::Docx, error))?;
    Ok(cursor.into_inner())
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="140" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="40"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/><w:spacing w:before="360" w:after="160"/></w:pPr><w:rPr><w:b/><w:color w:val="1B1A17"/><w:sz w:val="36"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/><w:spacing w:before="280" w:after="120"/></w:pPr><w:rPr><w:b/><w:color w:val="1B1A17"/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="2"/><w:spacing w:before="220" w:after="100"/></w:pPr><w:rPr><w:b/><w:color w:val="1B1A17"/><w:sz w:val="24"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="3"/><w:spacing w:before="200" w:after="80"/></w:pPr><w:rPr><w:b/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="4"/><w:spacing w:before="180" w:after="60"/></w:pPr><w:rPr><w:b/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="5"/><w:spacing w:before="160" w:after="60"/></w:pPr><w:rPr><w:b/><w:i/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:left w:val="single" w:sz="18" w:space="12" w:color="B9B7AE"/></w:pBdr><w:ind w:left="360"/><w:spacing w:before="120" w:after="160"/></w:pPr><w:rPr><w:i/><w:color w:val="4A4740"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F1F0EA"/><w:ind w:left="240" w:right="240"/><w:spacing w:before="140" w:after="160" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:sz w:val="19"/></w:rPr></w:style>
<w:style w:type="table" w:default="1" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:left w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:right w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="C9C9C2"/></w:tblBorders></w:tblPr></w:style>
</w:styles>"#;

const NUMBERING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="◦"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/></w:rPr></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="▪"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr></w:lvl>
<w:lvl w:ilvl="3"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2880" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="4"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="◦"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="3600" w:hanging="360"/></w:pPr></w:lvl>
</w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%3."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="3"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%4."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2880" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="4"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%5."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="3600" w:hanging="360"/></w:pPr></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        Document::new(vec![
            Block::Heading {
                level: 1,
                content: vec![Inline::Text("Release notes".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("A ".into()),
                Inline::Strong(vec![Inline::Text("bold".into())]),
                Inline::Text(" statement and a ".into()),
                Inline::Link {
                    content: vec![Inline::Text("link".into())],
                    href: "https://example.com".into(),
                },
                Inline::Text(".".into()),
            ]),
            Block::List {
                ordered: true,
                start: 1,
                items: vec![
                    ListItem::paragraph("First"),
                    ListItem::paragraph("Second"),
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
    fn round_trips_headings_paragraphs_lists_and_tables() {
        let document = sample();
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let back = READER.read(&bytes, &Options::default()).unwrap();
        assert!(back
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Heading { level: 1, .. })));
        assert!(back
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List { ordered: true, .. })));
        assert!(back
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table(_))));
        assert!(back.text_only().contains("bold"));
        assert!(back.text_only().contains("https://example.com") == false);
    }

    #[test]
    fn hyperlinks_survive_via_relationship() {
        let document = Document::new(vec![Block::Paragraph(vec![Inline::Link {
            content: vec![Inline::Text("site".into())],
            href: "https://example.com".into(),
        }])]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut rels = String::new();
        archive
            .by_name("word/_rels/document.xml.rels")
            .unwrap()
            .read_to_string(&mut rels)
            .unwrap();
        assert!(rels.contains("https://example.com"));
    }

    #[test]
    fn rejects_non_docx_zip() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let error = READER.read(&bytes, &Options::default()).unwrap_err();
        assert_eq!(error.code(), "wrong-format");
    }

    #[test]
    fn reads_tables_with_header_rows() {
        let xml = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let document = READER.read(&bytes, &Options::default()).unwrap();
        let Block::Table(table) = &document.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 1);
    }
}
