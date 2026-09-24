use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::diagnostics::escape_json_pointer_segment;

#[derive(Clone, Copy, Debug)]
struct ByteSpan {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceLocation {
    pub path: String,
    pub start: Position,
    pub end: Position,
}

impl SourceLocation {
    pub fn file_line(path: PathBuf, line: usize, start_column: usize, end_column: usize) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            start: Position {
                line,
                column: start_column,
            },
            end: Position {
                line,
                column: end_column,
            },
        }
    }
}

/// Locations in the original authored JSON, indexed by RFC 6901 pointer.
/// String mappings retain the encoded bytes for each decoded Unicode scalar.
pub struct SpanIndex {
    text: String,
    path: PathBuf,
    line_starts: Vec<usize>,
    values: HashMap<String, ByteSpan>,
    string_chars: HashMap<String, Vec<ByteSpan>>,
}

impl SpanIndex {
    pub fn new(text: &str, path: PathBuf) -> Result<Self, &'static str> {
        let mut index = Self {
            text: text.to_owned(),
            path,
            line_starts: std::iter::once(0)
                .chain(text.match_indices('\n').map(|(offset, _)| offset + 1))
                .collect(),
            values: HashMap::new(),
            string_chars: HashMap::new(),
        };
        let mut scanner = Scanner {
            text,
            cursor: 0,
            values: &mut index.values,
            string_chars: &mut index.string_chars,
        };
        scanner.value("")?;
        scanner.whitespace();
        if scanner.cursor != text.len() {
            return Err("extra data after the source document");
        }
        Ok(index)
    }

    pub fn location(&self, pointer: &str) -> SourceLocation {
        let mut candidate = pointer;
        loop {
            if let Some(span) = self.values.get(candidate) {
                return self.bytes_location(*span);
            }
            if let Some((parent, _)) = candidate.rsplit_once('/') {
                candidate = parent;
            } else {
                return self.bytes_location(ByteSpan { start: 0, end: 0 });
            }
        }
    }

    pub fn string_range(&self, pointer: &str, start: usize, end: usize) -> SourceLocation {
        let Some(chars) = self.string_chars.get(pointer) else {
            return self.location(pointer);
        };
        let Some(first) = chars.get(start) else {
            return self.location(pointer);
        };
        let Some(last) = end.checked_sub(1).and_then(|index| chars.get(index)) else {
            return self.location(pointer);
        };
        if end <= start {
            return self.location(pointer);
        }
        self.bytes_location(ByteSpan {
            start: first.start,
            end: last.end,
        })
    }

    fn bytes_location(&self, span: ByteSpan) -> SourceLocation {
        SourceLocation {
            path: self.path.to_string_lossy().into_owned(),
            start: self.position(span.start),
            end: self.position(span.end),
        }
    }

    fn position(&self, byte: usize) -> Position {
        let byte = byte.min(self.text.len());
        let line_index = self.line_starts.partition_point(|start| *start <= byte) - 1;
        Position {
            line: line_index + 1,
            column: self.text[self.line_starts[line_index]..byte]
                .chars()
                .count()
                + 1,
        }
    }
}

struct Scanner<'a> {
    text: &'a str,
    cursor: usize,
    values: &'a mut HashMap<String, ByteSpan>,
    string_chars: &'a mut HashMap<String, Vec<ByteSpan>>,
}

impl Scanner<'_> {
    fn whitespace(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), &'static str> {
        self.whitespace();
        if self.text.as_bytes().get(self.cursor) != Some(&byte) {
            return Err("could not index valid JSON source");
        }
        self.cursor += 1;
        Ok(())
    }

    fn value(&mut self, pointer: &str) -> Result<(), &'static str> {
        self.whitespace();
        let start = self.cursor;
        match self.text.as_bytes().get(self.cursor) {
            Some(b'{') => {
                self.cursor += 1;
                self.whitespace();
                if self.text.as_bytes().get(self.cursor) != Some(&b'}') {
                    loop {
                        let (key, _) = self.string()?;
                        self.expect(b':')?;
                        let child = format!("{pointer}/{}", escape_json_pointer_segment(&key));
                        self.value(&child)?;
                        self.whitespace();
                        if self.text.as_bytes().get(self.cursor) != Some(&b',') {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                self.expect(b'}')?;
            }
            Some(b'[') => {
                self.cursor += 1;
                self.whitespace();
                if self.text.as_bytes().get(self.cursor) != Some(&b']') {
                    let mut index = 0;
                    loop {
                        self.value(&format!("{pointer}/{index}"))?;
                        index += 1;
                        self.whitespace();
                        if self.text.as_bytes().get(self.cursor) != Some(&b',') {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                self.expect(b']')?;
            }
            Some(b'"') => {
                let (_, chars) = self.string()?;
                self.string_chars.insert(pointer.to_owned(), chars);
            }
            Some(_) => {
                while let Some(byte) = self.text.as_bytes().get(self.cursor) {
                    if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                        break;
                    }
                    self.cursor += 1;
                }
            }
            None => return Err("could not index valid JSON source"),
        }
        self.values.insert(
            pointer.to_owned(),
            ByteSpan {
                start,
                end: self.cursor,
            },
        );
        Ok(())
    }

    fn string(&mut self) -> Result<(String, Vec<ByteSpan>), &'static str> {
        self.whitespace();
        let start = self.cursor;
        self.expect(b'"')?;
        let mut chars = Vec::new();
        loop {
            let char_start = self.cursor;
            match self.text.as_bytes().get(self.cursor) {
                Some(b'"') => {
                    self.cursor += 1;
                    let decoded = serde_json::from_str(&self.text[start..self.cursor])
                        .map_err(|_| "could not decode JSON string while indexing spans")?;
                    return Ok((decoded, chars));
                }
                Some(b'\\') => {
                    let escape = *self
                        .text
                        .as_bytes()
                        .get(self.cursor + 1)
                        .ok_or("incomplete JSON string escape")?;
                    self.cursor += if escape == b'u' {
                        let hex = self
                            .text
                            .get(char_start + 2..char_start + 6)
                            .ok_or("incomplete JSON Unicode escape")?;
                        let unit = u16::from_str_radix(hex, 16)
                            .map_err(|_| "invalid JSON Unicode escape")?;
                        if (0xD800..=0xDBFF).contains(&unit) {
                            12
                        } else {
                            6
                        }
                    } else {
                        2
                    };
                }
                Some(_) => {
                    let ch = self.text[self.cursor..]
                        .chars()
                        .next()
                        .ok_or("invalid UTF-8 in JSON string")?;
                    self.cursor += ch.len_utf8();
                }
                None => return Err("unterminated JSON string"),
            }
            chars.push(ByteSpan {
                start: char_start,
                end: self.cursor,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_json_pointers_and_decoded_characters_to_encoded_spans() {
        let input =
            "{\n  \"blocks\": [{\"source\": {\"content\": \"a\\n\\uD83D\\uDE00 subgraph\"}}]\n}";
        let index = SpanIndex::new(input, PathBuf::from("/tmp/lesson.json")).unwrap();
        let pointer = "/blocks/0/source/content";
        let newline = index.string_range(pointer, 1, 2);
        assert_eq!(
            newline.start,
            Position {
                line: 2,
                column: 39
            }
        );
        assert_eq!(
            newline.end,
            Position {
                line: 2,
                column: 41
            }
        );
        let emoji = index.string_range(pointer, 2, 3);
        assert_eq!(emoji.end.column - emoji.start.column, 12);
        let keyword = index.string_range(pointer, 4, 12);
        assert_eq!(keyword.end.column - keyword.start.column, 8);
        assert_eq!(
            index.location("/blocks/0/absent"),
            index.location("/blocks/0")
        );
    }
}
