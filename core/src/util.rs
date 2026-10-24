//! Small, shared text helpers. Kept boring on purpose.

/// Map a Unicode scalar to its Windows-1252 byte, if it has one.
pub fn winansi_byte(character: char) -> Option<u8> {
    let code = character as u32;
    if (0x20..=0xFF).contains(&code) {
        // Windows-1252 leaves five Latin-1 positions undefined.
        return match code {
            0x81 | 0x8D | 0x8F | 0x90 | 0x9D => None,
            _ => Some(code as u8),
        };
    }
    let byte = match character {
        '\u{20AC}' => 0x80,
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85,
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2022}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        _ => return None,
    };
    Some(byte)
}

/// Inverse of [`winansi_byte`], used when reading legacy HTML and RTF.
pub fn winansi_char(byte: u8) -> char {
    match byte {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        // Undefined positions in cp1252 fall back to the C1 control, which is
        // better than silently eating the byte.
        _ => byte as char,
    }
}

pub fn decode_winansi(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| winansi_char(*byte)).collect()
}

pub fn is_winansi(character: char) -> bool {
    matches!(character, '\n' | '\t' | '\r') || winansi_byte(character).is_some()
}

/// Replace unmappable characters with `?`; used by PDF and RTF output.
pub fn lossy_winansi(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\n' | '\t' | '\r' => character,
            _ => {
                if winansi_byte(character).is_some() {
                    character
                } else {
                    '?'
                }
            }
        })
        .collect()
}

pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Collapse runs of whitespace to single spaces and trim the ends.
pub fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(character);
        }
    }
    out
}

/// Split a plain-text document into paragraphs at blank lines. Single newlines
/// inside a paragraph are kept as hard breaks, which makes TXT ↔ MD lossless.
pub fn plain_paragraphs(text: &str) -> Vec<String> {
    let text = normalize_newlines(text);
    let mut paragraphs = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(join_paragraph(&current));
                current.clear();
            }
        } else {
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        paragraphs.push(join_paragraph(&current));
    }
    paragraphs
}

fn join_paragraph(lines: &[&str]) -> String {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_start());
    }
    out
}

pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

/// XML 1.0 cannot carry most control characters; drop them rather than write an
/// invalid part.
pub fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(character),
            c if (c as u32) < 0x20 => {}
            _ => out.push(character),
        }
    }
    out
}

/// Escape text for a Markdown flow context, line by line so leading markers
/// cannot turn prose into a list or heading.
pub fn escape_markdown(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let mut escaped = escape_markdown_line(line);
        // A line that starts with an ordered-list marker.
        if let Some((digits, rest)) = split_ordered_marker(&escaped) {
            if !rest.is_empty() {
                escaped = format!("{digits}\\{rest}");
            }
        }
        lines.push(escaped);
    }
    lines.join("\\\n")
}

fn escape_markdown_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for (index, character) in line.chars().enumerate() {
        match character {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' => {
                out.push('\\');
                out.push(character);
            }
            '#' if index == 0 => out.push_str("\\#"),
            _ => out.push(character),
        }
    }
    out
}

fn split_ordered_marker(text: &str) -> Option<(String, String)> {
    let dot = text.find('.')?;
    if dot == 0 {
        return None;
    }
    if !text[..dot].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !text[dot + 1..].starts_with(' ') {
        return None;
    }
    Some((text[..dot].to_string(), text[dot..].to_string()))
}

/// RTF-safe text: braces and backslashes escaped, non-ASCII written as `\uN?`.
pub fn escape_rtf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\line "),
            _ => {
                if (character as u32) < 128 {
                    out.push(character);
                } else {
                    // RTF's \u takes a signed 16-bit value.
                    let value = character as i64;
                    if value <= 32767 {
                        out.push_str(&format!("\\u{value}?"));
                    } else if value <= 65535 {
                        let signed = value - 65536;
                        out.push_str(&format!("\\u{signed}?"));
                    } else {
                        out.push('?');
                    }
                }
            }
        }
    }
    out
}

/// Text between parentheses in a PDF literal string.
pub fn escape_pdf_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            '\r' => {}
            '\n' => out.push(' '),
            _ => {
                if winansi_byte(character).is_some() {
                    out.push(character);
                } else {
                    out.push('?');
                }
            }
        }
    }
    out
}

/// Remove HTML whitespace around inline fragments without destroying words.
pub fn trim_inline(text: &str) -> String {
    text.trim_matches(|c: char| c.is_whitespace()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winansi_covers_smart_punctuation() {
        assert_eq!(winansi_byte('\u{2019}'), Some(0x92));
        assert_eq!(winansi_byte('é'), Some(0xE9));
        assert_eq!(winansi_byte('東'), None);
        assert_eq!(lossy_winansi("café 東京"), "café ??");
    }

    #[test]
    fn paragraphs_keep_soft_breaks() {
        assert_eq!(
            plain_paragraphs("hello\nworld\r\n\r\nbye"),
            vec!["hello\nworld".to_string(), "bye".to_string()]
        );
    }

    #[test]
    fn markdown_escapes_structure() {
        assert_eq!(escape_markdown("# not a head"), "\\# not a head");
        assert_eq!(escape_markdown("1. not a list"), "1\\. not a list");
        assert_eq!(escape_markdown("a*b"), "a\\*b");
    }

    #[test]
    fn pdf_literal_escapes_parentheses() {
        assert_eq!(escape_pdf_literal("a(b)c\\"), "a\\(b\\)c\\\\");
    }

    #[test]
    fn html_and_xml_escape() {
        assert_eq!(escape_html("<a & b>"), "&lt;a &amp; b&gt;");
        assert_eq!(escape_xml("\u{1}<x>"), "&lt;x&gt;");
    }

    #[test]
    fn rtf_escapes_unicode() {
        assert_eq!(escape_rtf("é"), "\\u233?");
        assert_eq!(escape_rtf("{x}"), "\\{x\\}");
    }
}
