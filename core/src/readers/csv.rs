//! CSV in and out, mapped to the document's table block.

use crate::document::{inlines_plain, Align, Block, Document, Inline, Table};
use crate::error::ConvertError;
use crate::format::Format;
use crate::options::Options;
use crate::traits::{Reader, Writer};

pub struct CsvReader;
pub struct CsvWriter;

pub static READER: CsvReader = CsvReader;
pub static WRITER: CsvWriter = CsvWriter;

impl Reader for CsvReader {
    fn read(&self, input: &[u8], _options: &Options) -> Result<Document, ConvertError> {
        let input = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
        if std::str::from_utf8(input).is_err() {
            return Err(ConvertError::parse(
                Format::Csv,
                "CSV must be UTF-8; this file is in another encoding.",
            ));
        }
        let delimiter = detect_delimiter(input);
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(true)
            .flexible(true)
            .from_reader(input);

        let headers = reader
            .headers()
            .map_err(|error| ConvertError::parse(Format::Csv, error))?
            .iter()
            .map(cell)
            .collect::<Vec<_>>();

        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| ConvertError::parse(Format::Csv, error))?;
            rows.push(record.iter().map(cell).collect());
        }

        if headers.is_empty() && rows.is_empty() {
            return Ok(Document::default());
        }
        let columns = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(headers.len()))
            .max()
            .unwrap_or(0);
        let mut document = Document::new(vec![Block::Table(Table {
            headers,
            rows,
            align: vec![Align::Left; columns],
        })]);
        if delimiter != b',' {
            document.notices.push(format!(
                "Detected a {} delimiter.",
                match delimiter {
                    b'\t' => "tab",
                    b';' => "semicolon",
                    b'|' => "pipe",
                    _ => "custom",
                }
            ));
        }
        Ok(document)
    }
}

impl Writer for CsvWriter {
    fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError> {
        let table = document.first_table().cloned().unwrap_or_else(|| fallback_table(document));

        let columns = table.column_count().max(1);
        let headers: Vec<String> = (0..columns)
            .map(|index| {
                table
                    .headers
                    .get(index)
                    .map(|cell| inlines_plain(cell))
                    .unwrap_or_default()
            })
            .collect();
        let has_header = headers.iter().any(|header| !header.is_empty());

        let mut writer = csv::WriterBuilder::new()
            .delimiter(options.csv_delimiter)
            .terminator(csv::Terminator::Any(b'\n'))
            .quote_style(csv::QuoteStyle::Necessary)
            .from_writer(Vec::new());

        if has_header {
            writer
                .write_record(&headers)
                .map_err(|error| ConvertError::encode(Format::Csv, error))?;
        }
        for row in &table.rows {
            let cells: Vec<String> = (0..columns)
                .map(|index| {
                    row.get(index)
                        .map(|cell| inlines_plain(cell))
                        .unwrap_or_default()
                })
                .collect();
            writer
                .write_record(&cells)
                .map_err(|error| ConvertError::encode(Format::Csv, error))?;
        }
        writer
            .flush()
            .map_err(|error| ConvertError::encode(Format::Csv, error))?;
        writer
            .into_inner()
            .map_err(|error| ConvertError::encode(Format::Csv, error))
    }
}

fn cell(value: &str) -> Vec<Inline> {
    vec![Inline::Text(value.to_string())]
}

/// Documents without a table still deserve an export: one column, one row per
/// line of text. The UI warns that this is what happened.
fn fallback_table(document: &Document) -> Table {
    let text = document.text_only();
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(vec![cell(line)]);
    }
    Table {
        headers: Vec::new(),
        rows,
        align: vec![Align::Left],
    }
}

fn detect_delimiter(input: &[u8]) -> u8 {
    let text = String::from_utf8_lossy(input);
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut best = (b',', 0usize);
    for delimiter in [b',', b';', b'\t', b'|'] {
        let count = count_outside_quotes(first, delimiter);
        if count > best.1 {
            best = (delimiter, count);
        }
    }
    best.0
}

fn count_outside_quotes(line: &str, delimiter: u8) -> usize {
    let mut count = 0;
    let mut in_quotes = false;
    for byte in line.bytes() {
        match byte {
            b'"' => in_quotes = !in_quotes,
            byte if byte == delimiter && !in_quotes => count += 1,
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_headers_and_rows() {
        let document = READER
            .read(b"name,age\nAda,36\nGrace,45\n", &Options::default())
            .unwrap();
        let Block::Table(table) = &document.blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(
            document.text_only(),
            "name   age\nAda    36\nGrace  45\n"
        );
    }

    #[test]
    fn detects_semicolons_and_tabs() {
        assert_eq!(detect_delimiter(b"a;b;c\n1;2;3\n"), b';');
        assert_eq!(detect_delimiter(b"a\tb\n1\t2\n"), b'\t');
        assert_eq!(detect_delimiter(b"a,b\n1,2\n"), b',');
    }

    #[test]
    fn quotes_are_preserved() {
        let document = READER
            .read(b"name,note\nAda,\"a, b\"\n", &Options::default())
            .unwrap();
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "name,note\nAda,\"a, b\"\n");
    }

    #[test]
    fn writes_tables_from_other_documents() {
        let document = Document::new(vec![Block::Table(Table {
            headers: vec![vec![Inline::Text("a".into())], vec![Inline::Text("b".into())]],
            rows: vec![vec![
                vec![Inline::Text("1".into())],
                vec![Inline::Text("2".into())],
            ]],
            align: Vec::new(),
        })]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "a,b\n1,2\n");
    }

    #[test]
    fn falls_back_to_one_column() {
        let document = Document::new(vec![
            Block::Paragraph(vec![Inline::Text("first".into())]),
            Block::Paragraph(vec![Inline::Text("second".into())]),
        ]);
        let bytes = WRITER.write(&document, &Options::default()).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn rejects_non_utf8() {
        let error = READER.read(&[0xFF, 0xFE, 0x00], &Options::default()).unwrap_err();
        assert_eq!(error.code(), "parse");
    }
}
