//! The intermediate document model.
//!
//! Every reader produces a `Document`; every writer consumes one. That is the
//! whole reason the project does not need 64 hand-written converters.

use std::fmt::Write as _;

/// A parsed document, independent of any file format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
    /// Reader-side remarks (e.g. "images in this DOCX were skipped"). Writers
    /// must not persist these into the content; the app shows them as warnings.
    pub notices: Vec<String>,
}

/// A block-level element.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Inlines,
    },
    Paragraph(Inlines),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Table(Table),
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(Vec<Block>),
    Image(ImageRef),
    Rule,
}

pub type Inlines = Vec<Inline>;

/// An inline (run-level) element.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Strong(Inlines),
    Emphasis(Inlines),
    Strike(Inlines),
    Code(String),
    Link { content: Inlines, href: String },
    /// A hard line break inside the same paragraph.
    Break,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub headers: Vec<Inlines>,
    pub rows: Vec<Vec<Inlines>>,
    pub align: Vec<Align>,
}

/// An image reference. `data` is present only when the source carried the bytes
/// inline (DOCX media, HTML data URIs); remote URLs are never fetched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageRef {
    pub alt: String,
    pub mime: String,
    pub data: Option<Vec<u8>>,
    pub source: Option<String>,
}

impl Document {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            title: None,
            blocks,
            notices: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn text_only(&self) -> String {
        blocks_to_text(&self.blocks)
    }

    pub fn image_count(&self) -> usize {
        blocks_count_images(&self.blocks)
    }

    pub fn table_count(&self) -> usize {
        blocks_count_tables(&self.blocks)
    }

    pub fn has_non_winansi(&self) -> bool {
        blocks_have_non_winansi(&self.blocks)
    }

    /// First table in reading order, if any.
    pub fn first_table(&self) -> Option<&Table> {
        fn find<'a>(blocks: &'a [Block]) -> Option<&'a Table> {
            for block in blocks {
                match block {
                    Block::Table(table) => return Some(table),
                    Block::Quote(inner) => {
                        if let Some(table) = find(inner) {
                            return Some(table);
                        }
                    }
                    Block::List { items, .. } => {
                        for item in items {
                            if let Some(table) = find(&item.blocks) {
                                return Some(table);
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        find(&self.blocks)
    }

    /// True when the document is nothing but a single table (plus an optional
    /// title), which is what JSON exports as an array of objects.
    pub fn is_single_table(&self) -> bool {
        let mut tables = 0usize;
        let mut others = 0usize;
        fn walk(blocks: &[Block], tables: &mut usize, others: &mut usize) {
            for block in blocks {
                match block {
                    Block::Table(_) => *tables += 1,
                    Block::Rule => {}
                    Block::Quote(inner) => walk(inner, tables, others),
                    Block::List { items, .. } => {
                        for item in items {
                            walk(&item.blocks, tables, others);
                        }
                    }
                    _ => *others += 1,
                }
            }
        }
        walk(&self.blocks, &mut tables, &mut others);
        tables == 1 && others == 0
    }

    pub fn title_or(&self, fallback: &str) -> String {
        match &self.title {
            Some(title) if !title.trim().is_empty() => title.trim().to_string(),
            _ => fallback.to_string(),
        }
    }

    /// Counts shown in the app's structural preview.
    pub fn summary(&self) -> DocumentSummary {
        let mut summary = DocumentSummary {
            words: self.text_only().split_whitespace().count(),
            ..DocumentSummary::default()
        };
        summarise_blocks(&self.blocks, &mut summary);
        summary
    }
}

/// A cheap structural description of a [`Document`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentSummary {
    pub blocks: usize,
    pub paragraphs: usize,
    pub headings: usize,
    pub lists: usize,
    pub tables: usize,
    pub images: usize,
    pub code_blocks: usize,
    pub words: usize,
}

fn summarise_blocks(blocks: &[Block], summary: &mut DocumentSummary) {
    for block in blocks {
        summary.blocks += 1;
        match block {
            Block::Heading { .. } => summary.headings += 1,
            Block::Paragraph(_) => summary.paragraphs += 1,
            Block::List { items, .. } => {
                summary.lists += 1;
                for item in items {
                    summarise_blocks(&item.blocks, summary);
                }
            }
            Block::Table(_) => summary.tables += 1,
            Block::Code { .. } => summary.code_blocks += 1,
            Block::Quote(inner) => summarise_blocks(inner, summary),
            Block::Image(_) => summary.images += 1,
            Block::Rule => {}
        }
    }
}

impl Table {
    pub fn column_count(&self) -> usize {
        self.rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(self.headers.len()))
            .max()
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.rows.is_empty()
    }

    pub fn align_at(&self, index: usize) -> Align {
        self.align.get(index).copied().unwrap_or_default()
    }
}

impl Inline {
    pub fn text(value: impl Into<String>) -> Inline {
        Inline::Text(value.into())
    }

    pub fn plain(&self) -> String {
        let mut out = String::new();
        inline_plain(self, &mut out);
        out
    }
}

impl ListItem {
    pub fn paragraph(text: impl Into<String>) -> ListItem {
        ListItem {
            blocks: vec![Block::Paragraph(vec![Inline::Text(text.into())])],
        }
    }
}

pub fn inlines_plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        inline_plain(inline, &mut out);
    }
    out
}

fn inline_plain(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(text) => out.push_str(text),
        Inline::Strong(inner) | Inline::Emphasis(inner) | Inline::Strike(inner) => {
            for child in inner {
                inline_plain(child, out);
            }
        }
        Inline::Code(code) => out.push_str(code),
        Inline::Link { content, .. } => {
            for child in content {
                inline_plain(child, out);
            }
        }
        Inline::Break => out.push('\n'),
    }
}

fn blocks_to_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Heading { content, .. } => {
                out.push_str(&inlines_plain(content));
                out.push('\n');
            }
            Block::Paragraph(content) => {
                out.push_str(&inlines_plain(content));
                out.push('\n');
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                for (i, item) in items.iter().enumerate() {
                    if *ordered {
                        let number = start.saturating_add(i as u64);
                        let _ = write!(out, "{number}. ");
                    } else {
                        out.push_str("- ");
                    }
                    out.push_str(blocks_to_text(&item.blocks).trim_end());
                    out.push('\n');
                }
            }
            Block::Table(table) => {
                out.push_str(&table_to_text(table));
            }
            Block::Code { text, .. } => {
                out.push_str(text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            Block::Quote(inner) => {
                for line in blocks_to_text(inner).lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Block::Image(image) => {
                out.push_str(&format!(
                    "[image: {}]",
                    if image.alt.is_empty() {
                        "untitled"
                    } else {
                        &image.alt
                    }
                ));
                out.push('\n');
            }
            Block::Rule => out.push_str("---\n"),
        }
    }
    out
}

pub fn table_to_text(table: &Table) -> String {
    let columns = table.column_count();
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(
        (0..columns)
            .map(|i| {
                table
                    .headers
                    .get(i)
                    .map(|cell| inlines_plain(cell))
                    .unwrap_or_default()
            })
            .collect(),
    );
    for row in &table.rows {
        rows.push(
            (0..columns)
                .map(|i| row.get(i).map(|cell| inlines_plain(cell)).unwrap_or_default())
                .collect(),
        );
    }
    let mut widths = vec![0usize; columns];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(i) {
                *width = (*width).max(cell.chars().count());
            }
        }
    }
    let mut out = String::new();
    for row in &rows {
        let line = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let width = widths.get(i).copied().unwrap_or(0);
                format!("{cell:<width$}")
            })
            .collect::<Vec<_>>()
            .join("  ");
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn blocks_count_images(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Image(_) => 1,
            Block::Quote(inner) => blocks_count_images(inner),
            Block::List { items, .. } => items
                .iter()
                .map(|item| blocks_count_images(&item.blocks))
                .sum(),
            _ => 0,
        })
        .sum()
}

fn blocks_count_tables(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Table(_) => 1,
            Block::Quote(inner) => blocks_count_tables(inner),
            Block::List { items, .. } => items
                .iter()
                .map(|item| blocks_count_tables(&item.blocks))
                .sum(),
            _ => 0,
        })
        .sum()
}

fn blocks_have_non_winansi(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => inlines_have_non_winansi(content),
        Block::List { items, .. } => items
            .iter()
            .any(|item| blocks_have_non_winansi(&item.blocks)),
        Block::Table(table) => table
            .headers
            .iter()
            .chain(table.rows.iter().flatten())
            .any(|cell| inlines_have_non_winansi(cell)),
        Block::Code { text, .. } => !text.chars().all(crate::util::is_winansi),
        Block::Quote(inner) => blocks_have_non_winansi(inner),
        Block::Image(image) => !image.alt.chars().all(crate::util::is_winansi),
        Block::Rule => false,
    })
}

fn inlines_have_non_winansi(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text(text) | Inline::Code(text) => !text.chars().all(crate::util::is_winansi),
        Inline::Strong(inner) | Inline::Emphasis(inner) | Inline::Strike(inner) => {
            inlines_have_non_winansi(inner)
        }
        Inline::Link { content, href } => {
            inlines_have_non_winansi(content) || !href.chars().all(crate::util::is_winansi)
        }
        Inline::Break => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_follows_reading_order() {
        let doc = Document::new(vec![
            Block::Heading {
                level: 1,
                content: vec![Inline::Text("Title".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("Hello ".into()),
                Inline::Strong(vec![Inline::Text("world".into())]),
            ]),
        ]);
        assert_eq!(doc.text_only(), "Title\nHello world\n");
    }

    #[test]
    fn counts_nested_content() {
        let doc = Document::new(vec![Block::Quote(vec![
            Block::Image(ImageRef::default()),
            Block::Table(Table::default()),
        ])]);
        assert_eq!(doc.image_count(), 1);
        assert_eq!(doc.table_count(), 1);
    }

    #[test]
    fn single_table_detection() {
        let doc = Document::new(vec![Block::Table(Table {
            headers: vec![vec![Inline::Text("a".into())]],
            ..Table::default()
        })]);
        assert!(doc.is_single_table());
        let doc = Document::new(vec![Block::Paragraph(vec![Inline::Text("intro".into())])]);
        assert!(!doc.is_single_table());
    }

    #[test]
    fn non_winansi_detection() {
        let ok = Document::new(vec![Block::Paragraph(vec![Inline::Text("café".into())])]);
        assert!(!ok.has_non_winansi());
        let bad = Document::new(vec![Block::Paragraph(vec![Inline::Text("東京".into())])]);
        assert!(bad.has_non_winansi());
    }
}
