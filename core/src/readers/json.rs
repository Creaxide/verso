//! JSON as a structured document export.
//!
//! Three shapes are understood on the way in, and chosen deliberately on the
//! way out:
//! 1. a Verso document (`"format": "verso.document"`), lossless;
//! 2. an array of objects, exported for a lone table;
//! 3. anything else, kept as a JSON code block so nothing is silently dropped.

use serde_json::{Map, Value};

use crate::document::{
    inlines_plain, Align, Block, Document, ImageRef, Inline, Inlines, ListItem, Table,
};
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::traits::{Reader, Writer};

pub const FORMAT_MARKER: &str = "verso.document";
pub const FORMAT_VERSION: u64 = 1;

pub struct JsonReader;
pub struct JsonWriter;

pub static READER: JsonReader = JsonReader;
pub static WRITER: JsonWriter = JsonWriter;

impl Reader for JsonReader {
    fn read(&self, input: &[u8], _options: &Options) -> Result<Document, ConvertError> {
        let text = String::from_utf8_lossy(input);
        let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|error| ConvertError::parse(Format::Json, error))?;

        if let Value::Object(object) = &value {
            if object.contains_key("blocks")
                && (object.get("format").and_then(Value::as_str) == Some(FORMAT_MARKER)
                    || object.contains_key("version"))
            {
                return document_from_json(object);
            }
            let mut document = Document::new(vec![Block::Code {
                language: Some("json".to_string()),
                text: serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string()),
            }]);
            document.notices.push(
                "This JSON is not a Verso document or a table; it was kept as a JSON code block."
                    .to_string(),
            );
            return Ok(document);
        }

        if let Value::Array(items) = &value {
            if items.is_empty() {
                return Ok(Document::default());
            }
            if items.iter().all(Value::is_object) {
                return Ok(Document::new(vec![Block::Table(table_from_objects(items))]));
            }
            if items.iter().all(Value::is_array) {
                return Ok(Document::new(vec![Block::Table(table_from_arrays(items))]));
            }
            return Ok(Document::new(vec![Block::Table(table_from_scalars(items))]));
        }

        let mut document = Document::new(vec![Block::Code {
            language: Some("json".to_string()),
            text: serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string()),
        }]);
        document
            .notices
            .push("This JSON is a single value; it was kept as a JSON code block.".to_string());
        Ok(document)
    }
}

impl Writer for JsonWriter {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError> {
        let value = if document.is_single_table() {
            if let Some(table) = document.first_table() {
                table_to_value(table)
            } else {
                document_to_value(document, options)
            }
        } else {
            document_to_value(document, options)
        };
        let mut text = if options.json_pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        }
        .map_err(|error| ConvertError::encode(Format::Json, error))?;
        text.push('\n');
        Ok(text.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Document model
// ---------------------------------------------------------------------------

fn document_from_json(object: &Map<String, Value>) -> Result<Document, ConvertError> {
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string);
    let blocks = object
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().map(block_from_json).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(Document {
        title,
        blocks,
        notices: Vec::new(),
    })
}

fn block_from_json(value: &Value) -> Block {
    let Some(object) = value.as_object() else {
        return Block::Paragraph(vec![Inline::Text(value_to_text(value))]);
    };
    let kind = object
        .get("type")
        .or_else(|| object.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("paragraph");

    match kind {
        "heading" | "head" => Block::Heading {
            level: object
                .get("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as u8,
            content: inlines_from_json(
                object
                    .get("content")
                    .or_else(|| object.get("text"))
                    .unwrap_or(&Value::Null),
            ),
        },
        "paragraph" | "para" | "p" => Block::Paragraph(inlines_from_json(
            object
                .get("content")
                .or_else(|| object.get("text"))
                .unwrap_or(&Value::Null),
        )),
        "list" => {
            let ordered = object
                .get("ordered")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let start = object.get("start").and_then(Value::as_u64).unwrap_or(1);
            let items = object
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            if let Some(item_object) = item.as_object() {
                                if let Some(blocks) = item_object.get("blocks") {
                                    return ListItem {
                                        blocks: blocks
                                            .as_array()
                                            .map(|blocks| {
                                                blocks.iter().map(block_from_json).collect()
                                            })
                                            .unwrap_or_default(),
                                    };
                                }
                            }
                            if item.is_array() {
                                ListItem {
                                    blocks: item
                                        .as_array()
                                        .map(|blocks| blocks.iter().map(block_from_json).collect())
                                        .unwrap_or_default(),
                                }
                            } else {
                                ListItem {
                                    blocks: vec![Block::Paragraph(inlines_from_json(item))],
                                }
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Block::List {
                ordered,
                start,
                items,
            }
        }
        "table" => Block::Table(table_from_json(object)),
        "code" | "codeblock" => Block::Code {
            language: object
                .get("language")
                .or_else(|| object.get("lang"))
                .and_then(Value::as_str)
                .map(str::to_string),
            text: object
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        "quote" | "blockquote" => Block::Quote(
            object
                .get("blocks")
                .and_then(Value::as_array)
                .map(|blocks| blocks.iter().map(block_from_json).collect())
                .unwrap_or_default(),
        ),
        "image" => Block::Image(image_from_json(object)),
        "rule" | "hr" | "divider" => Block::Rule,
        _ => Block::Paragraph(inlines_from_json(
            object.get("content").unwrap_or(&Value::Null),
        )),
    }
}

fn inlines_from_json(value: &Value) -> Inlines {
    match value {
        Value::String(text) => vec![Inline::Text(text.clone())],
        Value::Array(items) => items.iter().flat_map(inlines_from_json).collect(),
        Value::Object(object) => {
            let kind = object
                .get("type")
                .or_else(|| object.get("t"))
                .and_then(Value::as_str);
            match kind {
                Some("text") | Some("t") => vec![Inline::Text(
                    object
                        .get("text")
                        .or_else(|| object.get("value"))
                        .or_else(|| object.get("v"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )],
                Some("strong") | Some("bold") => vec![Inline::Strong(inlines_from_json(
                    object.get("content").or_else(|| object.get("c")).unwrap_or(&Value::Null),
                ))],
                Some("emphasis") | Some("italic") | Some("em") => {
                    vec![Inline::Emphasis(inlines_from_json(
                        object.get("content").or_else(|| object.get("c")).unwrap_or(&Value::Null),
                    ))]
                }
                Some("strike") | Some("strikethrough") | Some("del") => {
                    vec![Inline::Strike(inlines_from_json(
                        object.get("content").or_else(|| object.get("c")).unwrap_or(&Value::Null),
                    ))]
                }
                Some("code") => vec![Inline::Code(
                    object
                        .get("text")
                        .or_else(|| object.get("value"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )],
                Some("link") | Some("a") => vec![Inline::Link {
                    content: inlines_from_json(
                        object.get("content").or_else(|| object.get("c")).unwrap_or(&Value::Null),
                    ),
                    href: object
                        .get("href")
                        .or_else(|| object.get("url"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                }],
                Some("break") | Some("br") => vec![Inline::Break],
                _ => inlines_from_json(
                    object
                        .get("content")
                        .or_else(|| object.get("text"))
                        .unwrap_or(&Value::Null),
                ),
            }
        }
        Value::Null => Vec::new(),
        other => vec![Inline::Text(value_to_text(other))],
    }
}

fn image_from_json(object: &Map<String, Value>) -> ImageRef {
    let data = object
        .get("data")
        .or_else(|| object.get("base64"))
        .and_then(Value::as_str)
        .and_then(|encoded| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(encoded).ok()
        });
    ImageRef {
        alt: object
            .get("alt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        mime: object
            .get("mime")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        data,
        source: object
            .get("source")
            .or_else(|| object.get("url"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn table_from_json(object: &Map<String, Value>) -> Table {
    let headers = object
        .get("headers")
        .and_then(Value::as_array)
        .map(|cells| cells.iter().map(inlines_from_json).collect())
        .unwrap_or_default();
    let rows = object
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .map(|cells| cells.iter().map(inlines_from_json).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let align = object
        .get("align")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| match item.as_str() {
                    Some("center") => Align::Center,
                    Some("right") => Align::Right,
                    _ => Align::Left,
                })
                .collect()
        })
        .unwrap_or_default();
    Table {
        headers,
        rows,
        align,
    }
}

fn document_to_value(document: &Document, options: &Options) -> Value {
    let mut root = Map::new();
    root.insert("format".to_string(), Value::String(FORMAT_MARKER.to_string()));
    root.insert("version".to_string(), Value::Number(FORMAT_VERSION.into()));
    if let Some(title) = &document.title {
        root.insert("title".to_string(), Value::String(title.clone()));
    }
    root.insert(
        "blocks".to_string(),
        Value::Array(
            document
                .blocks
                .iter()
                .map(|block| block_to_value(block, options))
                .collect(),
        ),
    );
    Value::Object(root)
}

fn block_to_value(block: &Block, options: &Options) -> Value {
    let mut object = Map::new();
    match block {
        Block::Heading { level, content } => {
            object.insert("type".to_string(), Value::String("heading".to_string()));
            object.insert("level".to_string(), Value::Number((*level).into()));
            object.insert("content".to_string(), inlines_to_value(content));
        }
        Block::Paragraph(content) => {
            object.insert("type".to_string(), Value::String("paragraph".to_string()));
            object.insert("content".to_string(), inlines_to_value(content));
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            object.insert("type".to_string(), Value::String("list".to_string()));
            object.insert("ordered".to_string(), Value::Bool(*ordered));
            if *ordered {
                object.insert("start".to_string(), Value::Number((*start).into()));
            }
            object.insert(
                "items".to_string(),
                Value::Array(
                    items
                        .iter()
                        .map(|item| {
                            let mut item_object = Map::new();
                            item_object.insert(
                                "blocks".to_string(),
                                Value::Array(
                                    item.blocks
                                        .iter()
                                        .map(|block| block_to_value(block, options))
                                        .collect(),
                                ),
                            );
                            Value::Object(item_object)
                        })
                        .collect(),
                ),
            );
        }
        Block::Table(table) => {
            object.insert("type".to_string(), Value::String("table".to_string()));
            object.insert(
                "headers".to_string(),
                Value::Array(table.headers.iter().map(|cell| inlines_to_value(cell)).collect()),
            );
            object.insert(
                "rows".to_string(),
                Value::Array(
                    table
                        .rows
                        .iter()
                        .map(|row| {
                            Value::Array(
                                row.iter()
                                    .map(|cell| inlines_to_value(cell))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect(),
                ),
            );
            object.insert(
                "align".to_string(),
                Value::Array(
                    table
                        .align
                        .iter()
                        .map(|align| {
                            Value::String(
                                match align {
                                    Align::Left => "left",
                                    Align::Center => "center",
                                    Align::Right => "right",
                                }
                                .to_string(),
                            )
                        })
                        .collect(),
                ),
            );
        }
        Block::Code { language, text } => {
            object.insert("type".to_string(), Value::String("code".to_string()));
            if let Some(language) = language {
                object.insert("language".to_string(), Value::String(language.clone()));
            }
            object.insert("text".to_string(), Value::String(text.clone()));
        }
        Block::Quote(inner) => {
            object.insert("type".to_string(), Value::String("quote".to_string()));
            object.insert(
                "blocks".to_string(),
                Value::Array(
                    inner
                        .iter()
                        .map(|block| block_to_value(block, options))
                        .collect(),
                ),
            );
        }
        Block::Image(image) => {
            object.insert("type".to_string(), Value::String("image".to_string()));
            object.insert("alt".to_string(), Value::String(image.alt.clone()));
            if let Some(data) = &image.data {
                if data.len() <= options.image_data_limit {
                    use base64::Engine as _;
                    object.insert(
                        "mime".to_string(),
                        Value::String(if image.mime.is_empty() {
                            "application/octet-stream".to_string()
                        } else {
                            image.mime.clone()
                        }),
                    );
                    object.insert(
                        "data".to_string(),
                        Value::String(base64::engine::general_purpose::STANDARD.encode(data)),
                    );
                } else {
                    object.insert(
                        "mime".to_string(),
                        Value::String(image.mime.clone()),
                    );
                }
            }
            if let Some(source) = &image.source {
                object.insert("source".to_string(), Value::String(source.clone()));
            }
        }
        Block::Rule => {
            object.insert("type".to_string(), Value::String("rule".to_string()));
        }
    }
    Value::Object(object)
}

fn inlines_to_value(inlines: &[Inline]) -> Value {
    Value::Array(
        inlines
            .iter()
            .map(|inline| {
                let mut object = Map::new();
                match inline {
                    Inline::Text(text) => {
                        object.insert("type".to_string(), Value::String("text".to_string()));
                        object.insert("text".to_string(), Value::String(text.clone()));
                    }
                    Inline::Strong(content) => {
                        object.insert("type".to_string(), Value::String("strong".to_string()));
                        object.insert("content".to_string(), inlines_to_value(content));
                    }
                    Inline::Emphasis(content) => {
                        object.insert(
                            "type".to_string(),
                            Value::String("emphasis".to_string()),
                        );
                        object.insert("content".to_string(), inlines_to_value(content));
                    }
                    Inline::Strike(content) => {
                        object.insert("type".to_string(), Value::String("strike".to_string()));
                        object.insert("content".to_string(), inlines_to_value(content));
                    }
                    Inline::Code(code) => {
                        object.insert("type".to_string(), Value::String("code".to_string()));
                        object.insert("text".to_string(), Value::String(code.clone()));
                    }
                    Inline::Link { content, href } => {
                        object.insert("type".to_string(), Value::String("link".to_string()));
                        object.insert("content".to_string(), inlines_to_value(content));
                        object.insert("href".to_string(), Value::String(href.clone()));
                    }
                    Inline::Break => {
                        object.insert("type".to_string(), Value::String("break".to_string()));
                    }
                }
                Value::Object(object)
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

fn table_to_value(table: &Table) -> Value {
    let columns = table.column_count();
    let headers: Vec<String> = (0..columns)
        .map(|index| {
            table
                .headers
                .get(index)
                .map(|cell| inlines_plain(cell))
                .unwrap_or_default()
        })
        .collect();
    let has_headers = headers.iter().any(|header| !header.is_empty());
    Value::Array(
        table
            .rows
            .iter()
            .map(|row| {
                if !has_headers {
                    return Value::Array(
                        (0..columns)
                            .map(|index| {
                                Value::String(
                                    row.get(index)
                                        .map(|cell| inlines_plain(cell))
                                        .unwrap_or_default(),
                                )
                            })
                            .collect(),
                    );
                }
                let mut object = Map::new();
                for index in 0..columns {
                    let key = unique_key(&headers, index);
                    let value = row
                        .get(index)
                        .map(|cell| inlines_plain(cell))
                        .unwrap_or_default();
                    object.insert(key, Value::String(value));
                }
                Value::Object(object)
            })
            .collect(),
    )
}

fn unique_key(headers: &[String], index: usize) -> String {
    let base = headers
        .get(index)
        .filter(|header| !header.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("column_{}", index + 1));
    let mut key = base.clone();
    let mut suffix = 2;
    while headers
        .iter()
        .take(index)
        .any(|existing| existing == &key)
    {
        key = format!("{base}_{suffix}");
        suffix += 1;
    }
    key
}

fn table_from_objects(items: &[Value]) -> Table {
    let mut keys: Vec<String> = Vec::new();
    for item in items {
        if let Some(object) = item.as_object() {
            for key in object.keys() {
                if !keys.iter().any(|existing| existing == key) {
                    keys.push(key.clone());
                }
            }
        }
    }
    let headers = keys
        .iter()
        .map(|key| vec![Inline::Text(key.clone())])
        .collect();
    let rows = items
        .iter()
        .map(|item| {
            keys.iter()
                .map(|key| {
                    let value = item.get(key).unwrap_or(&Value::Null);
                    vec![Inline::Text(value_to_text(value))]
                })
                .collect()
        })
        .collect();
    Table {
        headers,
        rows,
        align: Vec::new(),
    }
}

fn table_from_arrays(items: &[Value]) -> Table {
    let rows = items
        .iter()
        .map(|row| {
            row.as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|cell| vec![Inline::Text(value_to_text(cell))])
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    Table {
        headers: Vec::new(),
        rows,
        align: Vec::new(),
    }
}

fn table_from_scalars(items: &[Value]) -> Table {
    Table {
        headers: Vec::new(),
        rows: items
            .iter()
            .map(|item| vec![vec![Inline::Text(value_to_text(item))]])
            .collect(),
        align: Vec::new(),
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Right-pad rows so every writer sees a rectangle.
pub fn pad_cells(table: &Table) -> Vec<Vec<String>> {
    let columns = table.column_count();
    table
        .rows
        .iter()
        .map(|row| {
            (0..columns)
                .map(|index| {
                    row.get(index)
                        .map(|cell| inlines_plain(cell))
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect()
}

/// Kept for callers that build JSON text directly.
pub fn render(value: &Value, pretty: bool) -> Result<String, ConvertError> {
    let mut text = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|error| ConvertError::encode(Format::Json, error))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich_document() -> Document {
        Document::new(vec![
            Block::Heading {
                level: 1,
                content: vec![Inline::Text("Title".into())],
            },
            Block::Paragraph(vec![
                Inline::Text("Hello ".into()),
                Inline::Strong(vec![Inline::Text("bold".into())]),
                Inline::Text(" and ".into()),
                Inline::Emphasis(vec![Inline::Text("soft".into())]),
                Inline::Text(".".into()),
            ]),
            Block::List {
                ordered: true,
                start: 3,
                items: vec![
                    ListItem::paragraph("one"),
                    ListItem::paragraph("two"),
                ],
            },
            Block::Table(Table {
                headers: vec![
                    vec![Inline::Text("Name".into())],
                    vec![Inline::Text("Size".into())],
                ],
                rows: vec![vec![
                    vec![Inline::Text("a.txt".into())],
                    vec![Inline::Text("12".into())],
                ]],
                align: vec![Align::Left, Align::Right],
            }),
            Block::Code {
                language: Some("rust".into()),
                text: "let x = 1;".into(),
            },
            Block::Quote(vec![Block::Paragraph(vec![Inline::Text("quoted".into())])]),
            Block::Rule,
        ])
    }

    #[test]
    fn document_round_trips_through_json() {
        let document = rich_document();
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let back = READER.read(&bytes, &Options::default()).unwrap();
        assert_eq!(back.blocks, document.blocks);
    }

    #[test]
    fn a_lone_table_exports_as_objects() {
        let document = Document::new(vec![Block::Table(Table {
            headers: vec![
                vec![Inline::Text("name".into())],
                vec![Inline::Text("size".into())],
            ],
            rows: vec![vec![
                vec![Inline::Text("a".into())],
                vec![Inline::Text("1".into())],
            ]],
            align: Vec::new(),
        })]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.is_array());
        assert_eq!(value[0]["name"], Value::String("a".into()));
    }

    #[test]
    fn arrays_of_objects_become_tables() {
        let document = READER
            .read(br#"[{"a": 1, "b": "x"}, {"a": 2, "b": "y"}]"#, &Options::default())
            .unwrap();
        let Block::Table(table) = &document.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(document.text_only(), "a  b\n1  x\n2  y\n");
    }

    #[test]
    fn object_key_order_is_preserved() {
        let document = READER
            .read(
                br#"[{"frame": "01", "subject": "North window", "decision": "enlarge"}]"#,
                &Options::default(),
            )
            .unwrap();
        let Block::Table(table) = &document.blocks[0] else {
            panic!("expected table");
        };
        let headers: Vec<String> = table.headers.iter().map(|cell| inlines_plain(cell)).collect();
        assert_eq!(headers, ["frame", "subject", "decision"]);
    }

    #[test]
    fn generic_json_becomes_a_code_block() {
        let document = READER
            .read(br#"{"hello": "world"}"#, &Options::default())
            .unwrap();
        assert!(matches!(&document.blocks[0], Block::Code { .. }));
        assert!(!document.notices.is_empty());
    }
}
