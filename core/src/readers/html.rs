//! HTML reading on top of `scraper`'s html5ever tree.

use scraper::{ElementRef, Html, Node, Selector};

use crate::document::{inlines_plain, Align, Block, Document, ImageRef, Inline, Inlines, Table};
use crate::error::ConvertError;
use crate::options::Options;
use crate::traits::{Reader, Writer};
use crate::util::{collapse_whitespace, decode_winansi};

pub struct HtmlReader;

pub static READER: HtmlReader = HtmlReader;

impl Reader for HtmlReader {
    fn read(&self, input: &[u8], _options: &Options) -> Result<Document, ConvertError> {
        let text = decode_html(input);
        let html = Html::parse_document(&text);
        let title = Selector::parse("title")
            .ok()
            .and_then(|selector| html.select(&selector).next())
            .map(|element| collapse_whitespace(&element.text().collect::<String>()))
            .filter(|title| !title.is_empty());

        let mut blocks = Vec::new();
        let mut ctx = Context::default();
        if let Ok(selector) = Selector::parse("body") {
            if let Some(body) = html.select(&selector).next() {
                walk_children(body, &mut blocks, &mut ctx);
            } else {
                walk_element(html.root_element(), &mut blocks, &mut ctx);
            }
        } else {
            walk_element(html.root_element(), &mut blocks, &mut ctx);
        }
        // Images that did not sit inside a paragraph still belong to the page.
        drain_images(&mut blocks, &mut ctx);

        if let Some(note) = ctx.image_notice {
            // Reader notices are collected once even if many images were seen.
            ctx.notices.push(note);
        }
        Ok(Document {
            title,
            blocks,
            notices: ctx.notices,
        })
    }
}

/// Parse a raw HTML fragment (used for HTML embedded in Markdown).
pub fn read_fragment(html: &str) -> Result<Vec<Block>, ConvertError> {
    let document = Html::parse_fragment(html);
    let mut blocks = Vec::new();
    let mut ctx = Context::default();
    walk_element(document.root_element(), &mut blocks, &mut ctx);
    drain_images(&mut blocks, &mut ctx);
    Ok(blocks)
}

#[derive(Default)]
struct Context {
    notices: Vec<String>,
    image_notice: Option<String>,
    /// Images met while collecting inline runs, flushed as blocks after the
    /// paragraph they appeared in.
    pending_images: Vec<ImageRef>,
}

fn decode_html(input: &[u8]) -> String {
    let head = &input[..input.len().min(2048)];
    let sniff = String::from_utf8_lossy(head).to_ascii_lowercase();
    let declares_legacy = sniff.contains("charset=windows-1252")
        || sniff.contains("charset=iso-8859-1")
        || sniff.contains("charset=cp1252");
    if declares_legacy || std::str::from_utf8(input).is_err() {
        decode_winansi(input).trim_start_matches('\u{feff}').to_string()
    } else {
        String::from_utf8_lossy(input)
            .trim_start_matches('\u{feff}')
            .to_string()
    }
}

fn walk_element(element: ElementRef<'_>, out: &mut Vec<Block>, ctx: &mut Context) {
    let tag = element.value().name();
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag[1..].parse::<u8>().unwrap_or(1).clamp(1, 6);
            let content = collect_inlines(element, ctx);
            if !plain(&content).trim().is_empty() {
                out.push(Block::Heading { level, content });
            }
        }
        "p" => {
            let content = collect_inlines(element, ctx);
            push_paragraph(content, out, ctx);
        }
        "ul" | "ol" => {
            let ordered = tag == "ol";
            let start = element
                .value()
                .attr("start")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1);
            let mut items = Vec::new();
            for child in element.children() {
                if let Some(item) = ElementRef::wrap(child) {
                    if item.value().name() == "li" {
                        let mut blocks = Vec::new();
                        walk_children(item, &mut blocks, ctx);
                        if blocks.is_empty() {
                            let content = collect_inlines(item, ctx);
                            if !plain(&content).trim().is_empty() {
                                blocks.push(Block::Paragraph(content));
                            }
                        }
                        if !blocks.is_empty() {
                            items.push(crate::document::ListItem { blocks });
                        }
                    }
                }
            }
            if !items.is_empty() {
                out.push(Block::List {
                    ordered,
                    start,
                    items,
                });
            }
        }
        "blockquote" => {
            let mut inner = Vec::new();
            walk_children(element, &mut inner, ctx);
            if !inner.is_empty() {
                out.push(Block::Quote(inner));
            }
        }
        "pre" => {
            let text = element.text().collect::<String>();
            let language = element
                .children()
                .filter_map(ElementRef::wrap)
                .find(|child| child.value().name() == "code")
                .and_then(|code| language_from_class(code.value().attr("class")));
            let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
            while text.ends_with('\n') {
                text.pop();
            }
            out.push(Block::Code { language, text });
        }
        "table" => {
            if let Some(table) = read_table(element, ctx) {
                out.push(Block::Table(table));
            }
        }
        "hr" => out.push(Block::Rule),
        "img" => {
            let image = read_image(element, ctx);
            out.push(Block::Image(image));
        }
        "br" => {
            // A bare <br> between containers is a line break, not a block.
        }
        "script" | "style" | "noscript" | "template" | "head" | "meta" | "link" => {}
        "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside"
        | "figure" | "figcaption" | "body" | "html" | "dl" | "dt" | "dd" | "details"
        | "summary" | "form" | "fieldset" | "address" | "center" | "font" | "span" => {
            walk_children(element, out, ctx);
        }
        _ => {
            // Unknown containers still get their children read.
            walk_children(element, out, ctx);
        }
    }
}

fn walk_children(element: ElementRef<'_>, out: &mut Vec<Block>, ctx: &mut Context) {
    let mut pending: Inlines = Vec::new();
    let mut pending_space = false;

    for child in element.children() {
        match child.value() {
            Node::Text(text) => push_text(&mut pending, text, &mut pending_space),
            Node::Element(_) => {
                let Some(child) = ElementRef::wrap(child) else {
                    continue;
                };
                if is_block(child.value().name()) {
                    flush_inline(&mut pending, &mut pending_space, out, ctx);
                    walk_element(child, out, ctx);
                } else {
                    flush_space(&mut pending, &mut pending_space);
                    collect_inline_element(child, &mut pending, ctx);
                }
            }
            _ => {}
        }
    }
    flush_inline(&mut pending, &mut pending_space, out, ctx);
}

fn collect_inlines(element: ElementRef<'_>, ctx: &mut Context) -> Inlines {
    let mut out = Vec::new();
    let mut pending_space = false;
    for child in element.children() {
        match child.value() {
            Node::Text(text) => push_text(&mut out, text, &mut pending_space),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    flush_space(&mut out, &mut pending_space);
                    collect_inline_element(child, &mut out, ctx);
                }
            }
            _ => {}
        }
    }
    trim_edges(&mut out);
    out
}

fn collect_inline_element(element: ElementRef<'_>, out: &mut Inlines, ctx: &mut Context) {
    match element.value().name() {
        "strong" | "b" => {
            let content = collect_inlines(element, ctx);
            if !content.is_empty() {
                out.push(Inline::Strong(content));
            }
        }
        "em" | "i" => {
            let content = collect_inlines(element, ctx);
            if !content.is_empty() {
                out.push(Inline::Emphasis(content));
            }
        }
        "s" | "strike" | "del" => {
            let content = collect_inlines(element, ctx);
            if !content.is_empty() {
                out.push(Inline::Strike(content));
            }
        }
        "code" | "kbd" | "samp" => {
            let text = element.text().collect::<String>();
            if !text.is_empty() {
                out.push(Inline::Code(collapse_whitespace(&text)));
            }
        }
        "a" => {
            let href = element.value().attr("href").unwrap_or("");
            let content = collect_inlines(element, ctx);
            if !content.is_empty() {
                if href.is_empty() || href.starts_with('#') {
                    out.extend(content);
                } else {
                    out.push(Inline::Link {
                        content,
                        href: href.to_string(),
                    });
                }
            }
        }
        "br" => out.push(Inline::Break),
        "img" => {
            let image = read_image(element, ctx);
            ctx.pending_images.push(image);
        }
        "script" | "style" => {}
        _ => {
            let content = collect_inlines(element, ctx);
            out.extend(content);
        }
    }
}

fn read_image(element: ElementRef<'_>, ctx: &mut Context) -> ImageRef {
    let alt = element.value().attr("alt").unwrap_or("").to_string();
    let source = element.value().attr("src").unwrap_or("");
    let image = crate::readers::image_ref(&alt, source);
    if image.data.is_none() && ctx.image_notice.is_none() {
        ctx.image_notice = Some(
            "This page references images by URL. Only inline (data:) images can travel with the document."
                .to_string(),
        );
    }
    image
}

fn read_table(element: ElementRef<'_>, ctx: &mut Context) -> Option<Table> {
    let row_selector = Selector::parse("tr").ok()?;
    let cell_selector = Selector::parse("th, td").ok()?;

    let mut headers: Vec<Inlines> = Vec::new();
    let mut rows: Vec<Vec<Inlines>> = Vec::new();
    let mut align: Vec<Align> = Vec::new();
    let pending_before = ctx.pending_images.len();

    for (row_index, row) in element.select(&row_selector).enumerate() {
        let mut cells = Vec::new();
        let mut has_header = false;
        for cell in row.select(&cell_selector) {
            if cell.value().name() == "th" {
                has_header = true;
            }
            if row_index == 0 {
                align.push(read_align(cell.value().attr("align")));
            }
            cells.push(collect_inlines(cell, ctx));
        }
        if cells.is_empty() {
            continue;
        }
        if row_index == 0 && has_header {
            headers = cells;
        } else {
            rows.push(cells);
        }
    }

    if headers.is_empty() && rows.is_empty() {
        return None;
    }
    // A table cell cannot hold a block image; do not let one leak into the
    // next paragraph.
    ctx.pending_images.truncate(pending_before);
    let columns = rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(headers.len()))
        .max()
        .unwrap_or(0);
    align.resize(columns, Align::Left);
    Some(Table {
        headers,
        rows,
        align,
    })
}

fn read_align(value: Option<&str>) -> Align {
    match value.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("center") => Align::Center,
        Some("right") => Align::Right,
        _ => Align::Left,
    }
}

fn language_from_class(class: Option<&str>) -> Option<String> {
    let class = class?;
    for token in class.split_whitespace() {
        if let Some(language) = token.strip_prefix("language-") {
            if !language.is_empty() {
                return Some(language.to_string());
            }
        }
    }
    None
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "h1" | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "pre"
            | "table"
            | "hr"
            | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "figure"
            | "figcaption"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "form"
            | "fieldset"
            | "address"
            | "body"
            | "html"
            | "img"
    )
}

fn push_text(out: &mut Inlines, raw: &str, pending_space: &mut bool) {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        if !out.is_empty() {
            *pending_space = true;
        }
        return;
    }
    if raw.chars().next().is_some_and(char::is_whitespace) && !out.is_empty() {
        *pending_space = true;
    }
    flush_space(out, pending_space);
    out.push(Inline::Text(collapsed));
    if raw.ends_with(char::is_whitespace) {
        *pending_space = true;
    }
}

fn flush_space(out: &mut Inlines, pending_space: &mut bool) {
    if !*pending_space {
        return;
    }
    if let Some(Inline::Text(text)) = out.last_mut() {
        if !text.ends_with(' ') {
            text.push(' ');
            *pending_space = false;
            return;
        }
    }
    out.push(Inline::Text(" ".to_string()));
    *pending_space = false;
}

fn flush_inline(
    pending: &mut Inlines,
    pending_space: &mut bool,
    out: &mut Vec<Block>,
    ctx: &mut Context,
) {
    if *pending_space {
        *pending_space = false;
    }
    trim_edges(pending);
    if !pending.is_empty() {
        out.push(Block::Paragraph(std::mem::take(pending)));
    }
    pending.clear();
    drain_images(out, ctx);
}

fn push_paragraph(mut content: Inlines, out: &mut Vec<Block>, ctx: &mut Context) {
    trim_edges(&mut content);
    if !content.is_empty() {
        out.push(Block::Paragraph(content));
    }
    drain_images(out, ctx);
}

fn drain_images(out: &mut Vec<Block>, ctx: &mut Context) {
    for image in ctx.pending_images.drain(..) {
        out.push(Block::Image(image));
    }
}

fn trim_edges(inlines: &mut Inlines) {
    fn trim_start(inlines: &mut Inlines) -> bool {
        loop {
            let Some(first) = inlines.first_mut() else {
                return true;
            };
            let remove = match first {
                Inline::Text(text) => {
                    let trimmed = text.trim_start();
                    if trimmed.is_empty() {
                        true
                    } else {
                        *text = trimmed.to_string();
                        false
                    }
                }
                Inline::Break => true,
                Inline::Strong(content) | Inline::Emphasis(content) | Inline::Strike(content) => {
                    let empty = trim_start(content);
                    empty
                }
                Inline::Link { content, .. } => trim_start(content),
                Inline::Code(_) => false,
            };
            if remove {
                inlines.remove(0);
            } else {
                return false;
            }
        }
    }

    fn trim_end(inlines: &mut Inlines) {
        loop {
            let Some(last) = inlines.last_mut() else {
                return;
            };
            let remove = match last {
                Inline::Text(text) => {
                    let trimmed = text.trim_end();
                    if trimmed.is_empty() {
                        true
                    } else {
                        *text = trimmed.to_string();
                        false
                    }
                }
                Inline::Break => true,
                Inline::Strong(content) | Inline::Emphasis(content) | Inline::Strike(content) => {
                    trim_end(content);
                    content.is_empty()
                }
                Inline::Link { content, .. } => {
                    trim_end(content);
                    content.is_empty()
                }
                Inline::Code(_) => false,
            };
            if remove {
                inlines.pop();
            } else {
                return;
            }
        }
    }

    let _ = trim_start(inlines);
    trim_end(inlines);
}

fn plain(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(Inline::plain)
        .collect::<Vec<_>>()
        .join("")
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

pub struct HtmlWriter;

pub static WRITER: HtmlWriter = HtmlWriter;

const STYLE: &str = "\
:root { color-scheme: light; }
body { margin: 0; background: #fbfaf7; color: #1b1a17; font-family: Georgia, 'Times New Roman', serif; line-height: 1.65; }
main { max-width: 46rem; margin: 0 auto; padding: 3.5rem 1.5rem 5rem; }
h1, h2, h3, h4, h5, h6 { font-family: 'Helvetica Neue', Arial, sans-serif; line-height: 1.2; letter-spacing: -0.01em; }
h1 { font-size: 2.1rem; margin: 0 0 1.5rem; }
h2 { font-size: 1.6rem; margin: 2.4rem 0 .8rem; }
h3 { font-size: 1.25rem; margin: 1.9rem 0 .6rem; }
h4, h5, h6 { font-size: 1.05rem; margin: 1.6rem 0 .5rem; }
p { margin: 0 0 1.1rem; }
ul, ol { margin: 0 0 1.2rem; padding-left: 1.4rem; }
li { margin-bottom: .35rem; }
a { color: #7a4b21; text-underline-offset: 2px; }
table { border-collapse: collapse; width: 100%; margin: 1.5rem 0; font-family: 'Helvetica Neue', Arial, sans-serif; font-size: .92rem; }
th, td { border-bottom: 1px solid #dcd6c8; padding: .5rem .6rem; text-align: left; vertical-align: top; }
th { font-weight: 600; border-bottom-width: 2px; }
blockquote { margin: 1.5rem 0; padding-left: 1rem; border-left: 3px solid #d8d2c4; color: #4a463d; }
pre { background: #f1eee7; padding: 1rem; overflow-x: auto; border-radius: 4px; line-height: 1.45; }
code { font-family: 'SFMono-Regular', Consolas, monospace; font-size: .9em; }
img { max-width: 100%; height: auto; }
figure { margin: 1.5rem 0; }
figcaption { font-family: 'Helvetica Neue', Arial, sans-serif; font-size: .82rem; color: #6a655b; margin-top: .4rem; }
hr { border: 0; border-top: 1px solid #d8d2c4; margin: 2rem 0; }
.missing-image { font-family: 'Helvetica Neue', Arial, sans-serif; font-size: .85rem; color: #6a655b; }
";

impl Writer for HtmlWriter {
    fn write(&self, document: &Document, _options: &Options) -> Result<Vec<u8>, ConvertError> {
        let title = document
            .title
            .clone()
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            .or_else(|| first_heading(document))
            .unwrap_or_else(|| "Document".to_string());

        let mut out = String::with_capacity(4096);
        out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
        out.push_str("<meta charset=\"utf-8\">\n");
        out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
        out.push_str("<title>");
        out.push_str(&crate::util::escape_html(&title));
        out.push_str("</title>\n<style>\n");
        out.push_str(STYLE);
        out.push_str("</style>\n</head>\n<body>\n<main>\n");
        write_blocks(&mut out, &document.blocks);
        out.push_str("</main>\n</body>\n</html>\n");
        Ok(out.into_bytes())
    }
}

fn first_heading(document: &Document) -> Option<String> {
    fn find(blocks: &[Block]) -> Option<String> {
        for block in blocks {
            match block {
                Block::Heading { content, .. } => {
                    let text = inlines_plain(content);
                    if !text.trim().is_empty() {
                        return Some(text.trim().to_string());
                    }
                }
                Block::Quote(inner) => {
                    if let Some(text) = find(inner) {
                        return Some(text);
                    }
                }
                _ => {}
            }
        }
        None
    }
    find(&document.blocks)
}

fn write_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        write_block(out, block, 0);
    }
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    match block {
        Block::Heading { level, content } => {
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            write_inlines(out, content);
            out.push_str(&format!("</h{level}>\n"));
        }
        Block::Paragraph(content) => {
            out.push_str("<p>");
            write_inlines(out, content);
            out.push_str("</p>\n");
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            let tag = if *ordered { "ol" } else { "ul" };
            if *ordered && *start != 1 {
                out.push_str(&format!("<{tag} start=\"{start}\">\n"));
            } else {
                out.push_str(&format!("<{tag}>\n"));
            }
            for item in items {
                out.push_str("<li>");
                if item.blocks.len() == 1
                    && matches!(item.blocks.first(), Some(Block::Paragraph(_)))
                {
                    if let Some(Block::Paragraph(content)) = item.blocks.first() {
                        write_inlines(out, content);
                    }
                } else {
                    out.push('\n');
                    for child in &item.blocks {
                        write_block(out, child, depth + 1);
                    }
                }
                out.push_str("</li>\n");
            }
            out.push_str(&format!("</{tag}>\n"));
        }
        Block::Table(table) => {
            out.push_str("<table>\n");
            if !table.headers.is_empty() {
                out.push_str("<thead>\n<tr>");
                for (index, cell) in table.headers.iter().enumerate() {
                    out.push_str("<th");
                    write_align(out, table.align_at(index));
                    out.push('>');
                    write_inlines(out, cell);
                    out.push_str("</th>");
                }
                out.push_str("</tr>\n</thead>\n");
            }
            if !table.rows.is_empty() {
                out.push_str("<tbody>\n");
                for row in &table.rows {
                    out.push_str("<tr>");
                    for (index, cell) in row.iter().enumerate() {
                        out.push_str("<td");
                        write_align(out, table.align_at(index));
                        out.push('>');
                        write_inlines(out, cell);
                        out.push_str("</td>");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</tbody>\n");
            }
            out.push_str("</table>\n");
        }
        Block::Code { language, text } => {
            out.push_str("<pre><code");
            if let Some(language) = language.as_deref().filter(|value| !value.is_empty()) {
                out.push_str(" class=\"language-");
                out.push_str(&crate::util::escape_html(language));
                out.push('"');
            }
            out.push('>');
            out.push_str(&crate::util::escape_html(text.trim_end_matches('\n')));
            out.push_str("</code></pre>\n");
        }
        Block::Quote(inner) => {
            out.push_str("<blockquote>\n");
            for child in inner {
                write_block(out, child, depth + 1);
            }
            out.push_str("</blockquote>\n");
        }
        Block::Image(image) => write_image(out, image),
        Block::Rule => out.push_str("<hr>\n"),
    }
}

fn write_align(out: &mut String, align: Align) {
    match align {
        Align::Left => {}
        Align::Center => out.push_str(" style=\"text-align: center\""),
        Align::Right => out.push_str(" style=\"text-align: right\""),
    }
}

fn write_image(out: &mut String, image: &ImageRef) {
    out.push_str("<figure>");
    let alt = crate::util::escape_html(&image.alt);
    if let Some(data) = &image.data {
        use base64::Engine as _;
        let mime = if image.mime.trim().is_empty() {
            "application/octet-stream"
        } else {
            image.mime.trim()
        };
        out.push_str("<img src=\"data:");
        out.push_str(&crate::util::escape_html(mime));
        out.push_str(";base64,");
        out.push_str(&base64::engine::general_purpose::STANDARD.encode(data));
        out.push_str("\" alt=\"");
        out.push_str(&alt);
        out.push_str("\">");
    } else if let Some(source) = image.source.as_deref().filter(|value| !value.is_empty()) {
        out.push_str("<img src=\"");
        out.push_str(&crate::util::escape_html(source));
        out.push_str("\" alt=\"");
        out.push_str(&alt);
        out.push_str("\">");
    } else {
        out.push_str("<span class=\"missing-image\">[image: ");
        out.push_str(if image.alt.trim().is_empty() {
            "untitled"
        } else {
            &alt
        });
        out.push_str("]</span>");
    }
    if !image.alt.trim().is_empty() {
        out.push_str("<figcaption>");
        out.push_str(&alt);
        out.push_str("</figcaption>");
    }
    out.push_str("</figure>\n");
}

fn write_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&crate::util::escape_html(text)),
            Inline::Strong(inner) => {
                out.push_str("<strong>");
                write_inlines(out, inner);
                out.push_str("</strong>");
            }
            Inline::Emphasis(inner) => {
                out.push_str("<em>");
                write_inlines(out, inner);
                out.push_str("</em>");
            }
            Inline::Strike(inner) => {
                out.push_str("<del>");
                write_inlines(out, inner);
                out.push_str("</del>");
            }
            Inline::Code(code) => {
                out.push_str("<code>");
                out.push_str(&crate::util::escape_html(code));
                out.push_str("</code>");
            }
            Inline::Link { content, href } => {
                out.push_str("<a href=\"");
                out.push_str(&crate::util::escape_html(href));
                out.push_str("\">");
                write_inlines(out, content);
                out.push_str("</a>");
            }
            Inline::Break => out.push_str("<br>\n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(html: &str) -> Document {
        READER.read(html.as_bytes(), &Options::default()).unwrap()
    }

    #[test]
    fn reads_title_headings_and_paragraphs() {
        let doc = read(
            "<html><head><title>Report</title></head><body><h1>Hello</h1><p>One <b>bold</b> word.</p></body></html>",
        );
        assert_eq!(doc.title.as_deref(), Some("Report"));
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
        assert_eq!(doc.text_only(), "Hello\nOne bold word.\n");
    }

    #[test]
    fn reads_lists_and_links() {
        let doc = read("<ul><li>One</li><li><a href='https://x.test'>Two</a></li></ul>");
        let Block::List { items, .. } = &doc.blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(
            &items[1].blocks[0],
            Block::Paragraph(inlines) if matches!(inlines[0], Inline::Link { .. })
        ));
    }

    #[test]
    fn reads_tables_with_alignment() {
        let doc = read(
            "<table><tr><th align='right'>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
        );
        let Block::Table(table) = &doc.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.align_at(0), Align::Right);
    }

    #[test]
    fn reads_preformatted_code() {
        let doc = read("<pre><code class='language-rust'>let x = 1;\n</code></pre>");
        let Block::Code { language, text } = &doc.blocks[0] else {
            panic!("expected code");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "let x = 1;");
    }

    #[test]
    fn collapses_whitespace() {
        let doc = read("<p>one\n   two <i>three</i>  four</p>");
        assert_eq!(doc.text_only(), "one two three four\n");
    }

    #[test]
    fn decodes_cp1252_pages() {
        let mut html = b"<html><head><meta charset=\"windows-1252\"></head><body><p>caf".to_vec();
        html.push(0xE9);
        html.extend_from_slice(b"</p></body></html>");
        let doc = READER.read(&html, &Options::default()).unwrap();
        assert_eq!(doc.text_only(), "café\n");
    }
}
