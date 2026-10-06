//! Tokenizer for PDF syntax: numbers, names, strings, dicts, arrays,
//! keywords. Whitespace- and comment-tolerant.

use crate::ParseError;
use std::ops::Range;

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Integer (no decimal point seen).
    Int(i64),
    /// Real number.
    Real(f64),
    /// Name object, stored without the leading slash.
    Name(String),
    /// Literal string `( ... )`.
    LiteralString(Vec<u8>),
    /// Hex string `< ... >`.
    HexString(Vec<u8>),
    /// `<<`.
    DictOpen,
    /// `>>`.
    DictClose,
    /// `[`.
    ArrayOpen,
    /// `]`.
    ArrayClose,
    /// `R` (the reference keyword).
    RefKeyword,
    /// `obj` keyword.
    ObjKeyword,
    /// `endobj` keyword.
    EndObjKeyword,
    /// `stream` keyword.
    StreamKeyword,
    /// `endstream` keyword.
    EndStreamKeyword,
    /// `xref` keyword.
    XrefKeyword,
    /// `trailer` keyword.
    TrailerKeyword,
    /// `true`.
    True,
    /// `false`.
    False,
    /// `null`.
    Null,
    /// Anything unexpected; carries the offending byte.
    Other(u8),
}

/// Whitespace and delimiter bytes.
fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | b' ')
}

/// Streaming lexer over a byte slice.
pub struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// New lexer at byte 0.
    #[must_use] 
    pub fn new(data: &'a [u8]) -> Self {
        Lexer { data, pos: 0 }
    }

    /// Current byte offset.
    #[must_use] 
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Jump to a byte offset.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
    }

    /// Skip whitespace and `%`-comments.
    pub fn skip_trivia(&mut self) {
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if is_whitespace(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self.pos < self.data.len() && self.data[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Peek at the next non-trivia byte without consuming it.
    pub fn peek(&mut self) -> Option<u8> {
        self.skip_trivia();
        self.data.get(self.pos).copied()
    }

    fn read_number(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        let mut saw_point = false;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if b.is_ascii_digit() {
                self.pos += 1;
            } else if (b == b'.' || b == b'-') && !saw_point && self.pos == start {
                if b == b'.' {
                    saw_point = true;
                }
                self.pos += 1;
            } else if b == b'.' {
                saw_point = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.data[start..self.pos]).unwrap_or("");
        if text.is_empty() || text == "." {
            return Err(ParseError::Malformed(start, "empty number".into()));
        }
        if saw_point {
            Ok(Token::Real(text.parse::<f64>().map_err(|_| {
                ParseError::Malformed(start, format!("bad real {text:?}"))
            })?))
        } else {
            Ok(Token::Int(text.parse::<i64>().map_err(|_| {
                ParseError::Malformed(start, format!("bad int {text:?}"))
            })?))
        }
    }

    fn read_name(&mut self) -> Token {
        self.pos += 1; // consume '/'
        let start = self.pos;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if is_whitespace(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        // Unescape #XX sequences.
        let raw = &self.data[start..self.pos];
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'#' && i + 2 < raw.len() {
                if let Ok(v) = u8::from_str_radix(&String::from_utf8_lossy(&raw[i + 1..i + 3]), 16)
                {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(raw[i]);
            i += 1;
        }
        Token::Name(String::from_utf8_lossy(&out).into_owned())
    }

    fn read_literal_string(&mut self) -> Result<Token, ParseError> {
        self.pos += 1; // consume '('
        let mut depth = 1;
        let mut out = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(Token::LiteralString(out));
                    }
                    out.push(b);
                }
                b'\\' => {
                    self.pos += 1;
                    if self.pos >= self.data.len() {
                        break;
                    }
                    match self.data[self.pos] {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        c @ (b'(' | b')' | b'\\') => out.push(c),
                        c if c.is_ascii_digit() => {
                            // Up to 3 octal digits.
                            let mut v = 0u32;
                            let mut n = 0;
                            while n < 3
                                && self.pos < self.data.len()
                                && self.data[self.pos].is_ascii_digit()
                            {
                                v = v * 8 + u32::from(self.data[self.pos] - b'0');
                                self.pos += 1;
                                n += 1;
                            }
                            out.push((v & 0xff) as u8);
                            continue;
                        }
                        // Line continuation: skip EOL.
                        _ => {}
                    }
                }
                _ => out.push(b),
            }
            self.pos += 1;
        }
        Err(ParseError::Malformed(
            self.pos,
            "unterminated string".into(),
        ))
    }

    fn read_hex_string(&mut self) -> Result<Token, ParseError> {
        // `<...>`: consume until '>'; `<<` was already handled by caller.
        let start = self.pos;
        self.pos += 1;
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;
        while self.pos < self.data.len() && self.data[self.pos] != b'>' {
            let b = self.data[self.pos];
            if let Some(v) = (b as char).to_digit(16) {
                match hi {
                    None => hi = Some(v as u8),
                    Some(h) => {
                        out.push(h << 4 | v as u8);
                        hi = None;
                    }
                }
            }
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            return Err(ParseError::Malformed(
                start,
                "unterminated hex string".into(),
            ));
        }
        if let Some(h) = hi {
            out.push(h << 4); // odd digit count: last high nibble + 0
        }
        self.pos += 1; // consume '>'
        Ok(Token::HexString(out))
    }

    fn read_keyword(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if is_whitespace(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        match &self.data[start..self.pos] {
            b"R" => Ok(Token::RefKeyword),
            b"obj" => Ok(Token::ObjKeyword),
            b"endobj" => Ok(Token::EndObjKeyword),
            b"stream" => Ok(Token::StreamKeyword),
            b"endstream" => Ok(Token::EndStreamKeyword),
            b"xref" => Ok(Token::XrefKeyword),
            b"trailer" => Ok(Token::TrailerKeyword),
            b"true" => Ok(Token::True),
            b"false" => Ok(Token::False),
            b"null" => Ok(Token::Null),
            _ => Err(ParseError::Malformed(
                start,
                format!(
                    "unknown keyword {:?}",
                    String::from_utf8_lossy(&self.data[start..self.pos])
                ),
            )),
        }
    }

    /// Read the next token (skipping whitespace/comments). `Ok(None)` at EOF.
    pub fn next_token(&mut self) -> Result<Option<Token>, ParseError> {
        let Some(b) = self.peek() else {
            return Ok(None);
        };
        match b {
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.read_number().map(Some),
            b'/' => Ok(Some(self.read_name())),
            b'(' => self.read_literal_string().map(Some),
            b'[' => {
                self.pos += 1;
                Ok(Some(Token::ArrayOpen))
            }
            b']' => {
                self.pos += 1;
                Ok(Some(Token::ArrayClose))
            }
            b'<' => {
                if self.data.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Ok(Some(Token::DictOpen))
                } else {
                    self.read_hex_string().map(Some)
                }
            }
            b'>' => {
                if self.data.get(self.pos + 1) == Some(&b'>') {
                    self.pos += 2;
                    Ok(Some(Token::DictClose))
                } else {
                    Err(ParseError::Malformed(self.pos, "stray '>'".into()))
                }
            }
            b if b.is_ascii_alphabetic() => self.read_keyword().map(Some),
            other => {
                self.pos += 1;
                Ok(Some(Token::Other(other)))
            }
        }
    }

    /// Read a stream's raw bytes given the byte range of the body.
    #[must_use] 
    pub fn stream_body(&self, range: Range<usize>) -> &[u8] {
        &self.data[range]
    }
}
