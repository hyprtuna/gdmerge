//! Tokenizer for the Godot text-resource variant syntax.
//!
//! Mirrors `VariantParser::get_token` in Godot's `core/variant/variant_parser.cpp`:
//! the same single-character tokens, the same string escapes, the same number
//! shapes, and `;` line comments.

use std::fmt;
use std::ops::Range;

use crate::error::{ParseError, ParseErrorKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Comma,
    Period,
    Equal,
    /// `#rrggbb` / `#rrggbbaa` colour literal, stored without the `#`.
    Color(String),
    /// A quoted string. `name` is true for the `&"..."` / `@"..."` StringName form.
    Str {
        value: String,
        name: bool,
    },
    Num(f64),
    Ident(String),
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::LBrace => f.write_str("'{'"),
            Tok::RBrace => f.write_str("'}'"),
            Tok::LBracket => f.write_str("'['"),
            Tok::RBracket => f.write_str("']'"),
            Tok::LParen => f.write_str("'('"),
            Tok::RParen => f.write_str("')'"),
            Tok::Colon => f.write_str("':'"),
            Tok::Comma => f.write_str("','"),
            Tok::Period => f.write_str("'.'"),
            Tok::Equal => f.write_str("'='"),
            Tok::Color(_) => f.write_str("a colour literal"),
            Tok::Str { .. } => f.write_str("a string"),
            Tok::Num(_) => f.write_str("a number"),
            Tok::Ident(_) => f.write_str("an identifier"),
            Tok::Eof => f.write_str("end of file"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Range<usize>,
}

pub struct Lexer<'a> {
    pub(crate) src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer { src, bytes: src.as_bytes(), pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Byte offset the lexer will resume from. Used to slice raw source text.
    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn err(&self, at: usize, kind: ParseErrorKind) -> ParseError {
        ParseError::new(self.src, at, kind)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.src[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Skips whitespace and `;` line comments. Leaves `pos` on the next token.
    pub fn skip_trivia(&mut self) {
        loop {
            match self.peek_byte() {
                Some(b) if b <= 32 => self.pos += 1,
                Some(b';') => {
                    while let Some(b) = self.peek_byte() {
                        self.pos += 1;
                        if b == b'\n' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Token, ParseError> {
        self.skip_trivia();
        let start = self.pos;
        let Some(c) = self.next_char() else {
            return Ok(Token { tok: Tok::Eof, span: start..start });
        };
        let tok = match c {
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ':' => Tok::Colon,
            ',' => Tok::Comma,
            '.' => Tok::Period,
            '=' => Tok::Equal,
            '#' => {
                let hex_start = self.pos;
                while matches!(self.peek_byte(), Some(b) if b.is_ascii_hexdigit()) {
                    self.pos += 1;
                }
                Tok::Color(self.src[hex_start..self.pos].to_string())
            }
            '&' | '@' => {
                if self.peek_byte() != Some(b'"') {
                    return Err(self.err(start, ParseErrorKind::ExpectedQuoteAfterAmpersand));
                }
                self.pos += 1;
                Tok::Str { value: self.lex_string_body(start)?, name: true }
            }
            '"' => Tok::Str { value: self.lex_string_body(start)?, name: false },
            // Godot lexes `-` as part of the following number *or* identifier,
            // which is how `-inf` reaches `parse_value` as the identifier "-inf".
            '-' => match self.peek_byte() {
                Some(b) if b.is_ascii_digit() => {
                    self.pos = start;
                    Tok::Num(self.lex_number()?)
                }
                Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                    Tok::Ident(self.lex_ident_body(start))
                }
                _ => return Err(self.err(start, ParseErrorKind::UnexpectedCharacter('-'))),
            },
            '0'..='9' => {
                self.pos = start;
                Tok::Num(self.lex_number()?)
            }
            c if c.is_ascii_alphabetic() || c == '_' => Tok::Ident(self.lex_ident_body(start)),
            _ => return Err(self.err(start, ParseErrorKind::UnexpectedCharacter(c))),
        };
        Ok(Token { tok, span: start..self.pos })
    }

    /// Consumes the rest of an identifier and returns the whole slice from `start`.
    fn lex_ident_body(&mut self, start: usize) -> String {
        while matches!(self.peek_byte(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
            self.pos += 1;
        }
        self.src[start..self.pos].to_string()
    }

    /// Reads a string body, with `pos` just past the opening quote.
    fn lex_string_body(&mut self, start: usize) -> Result<String, ParseError> {
        let mut out = String::new();
        // Pending UTF-16 lead surrogate, as Godot's parser tracks in `prev`.
        let mut lead: Option<u32> = None;
        loop {
            let Some(c) = self.next_char() else {
                return Err(self.err(start, ParseErrorKind::UnterminatedString));
            };
            if c == '"' {
                break;
            }
            if c != '\\' {
                if lead.is_some() {
                    return Err(self.err(start, ParseErrorKind::UnpairedSurrogate));
                }
                out.push(c);
                continue;
            }
            let Some(esc) = self.next_char() else {
                return Err(self.err(start, ParseErrorKind::UnterminatedString));
            };
            let mut res: u32 = match esc {
                'b' => 8,
                't' => 9,
                'n' => 10,
                'f' => 12,
                'r' => 13,
                'u' | 'U' => {
                    let len = if esc == 'U' { 6 } else { 4 };
                    let mut v: u32 = 0;
                    for _ in 0..len {
                        let at = self.pos;
                        let Some(h) = self.next_char() else {
                            return Err(self.err(start, ParseErrorKind::UnterminatedString));
                        };
                        let Some(d) = h.to_digit(16) else {
                            return Err(self.err(at, ParseErrorKind::MalformedHexEscape));
                        };
                        v = (v << 4) | d;
                    }
                    v
                }
                other => other as u32,
            };
            if (res & 0xffff_fc00) == 0xd800 {
                if lead.is_some() {
                    return Err(self.err(start, ParseErrorKind::UnpairedSurrogate));
                }
                lead = Some(res);
                continue;
            } else if (res & 0xffff_fc00) == 0xdc00 {
                let Some(hi) = lead.take() else {
                    return Err(self.err(start, ParseErrorKind::UnpairedSurrogate));
                };
                res = (hi << 10).wrapping_add(res).wrapping_sub((0xd800 << 10) + 0xdc00 - 0x1_0000);
            } else if lead.is_some() {
                return Err(self.err(start, ParseErrorKind::UnpairedSurrogate));
            }
            match char::from_u32(res) {
                Some(ch) => out.push(ch),
                None => return Err(self.err(start, ParseErrorKind::UnpairedSurrogate)),
            }
        }
        if lead.is_some() {
            return Err(self.err(start, ParseErrorKind::UnpairedSurrogate));
        }
        Ok(out)
    }

    fn lex_number(&mut self) -> Result<f64, ParseError> {
        let start = self.pos;
        if self.peek_byte() == Some(b'-') {
            self.pos += 1;
        }
        // Integer part.
        while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek_byte() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek_byte(), Some(b'e') | Some(b'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'-') | Some(b'+')) {
                self.pos += 1;
            }
            if matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
                while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }
        let text = &self.src[start..self.pos];
        text.parse::<f64>().map_err(|_| self.err(start, ParseErrorKind::MalformedNumber))
    }
}

/// One-token-lookahead cursor over a [`Lexer`], plus the byte bookkeeping the
/// document parser needs to slice raw source text back out.
pub struct Cursor<'a> {
    lex: Lexer<'a>,
    peeked: Option<Token>,
    last_end: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(src: &'a str) -> Self {
        Cursor { lex: Lexer::new(src), peeked: None, last_end: 0 }
    }

    pub fn src(&self) -> &'a str {
        self.lex.src
    }

    pub fn next(&mut self) -> Result<Token, ParseError> {
        let tok = match self.peeked.take() {
            Some(t) => t,
            None => self.lex.next_token()?,
        };
        self.last_end = tok.span.end;
        Ok(tok)
    }

    pub fn peek(&mut self) -> Result<&Token, ParseError> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex.next_token()?);
        }
        Ok(self.peeked.as_ref().expect("just filled"))
    }

    /// Byte offset just past the most recently consumed token.
    pub fn end(&self) -> usize {
        self.last_end
    }

    /// Byte offset where the next token begins, skipping trivia.
    pub fn next_start(&mut self) -> Result<usize, ParseError> {
        Ok(self.peek()?.span.start)
    }

    /// Byte offset of the first non-trivia byte, without tokenizing it. Unlike
    /// [`Cursor::next_start`] this cannot fail on a malformed token.
    pub fn skip_trivia_pos(&mut self) -> usize {
        if let Some(t) = &self.peeked {
            return t.span.start;
        }
        self.lex.skip_trivia();
        self.lex.pos()
    }

    /// Rewinds to just past the last consumed token, discarding any lookahead.
    ///
    /// Value parsing peeks one token past the value it returns. At an item
    /// boundary that lookahead can land on a property name such as `%anim` that
    /// is not a valid *value* token, so the document parser rewinds before
    /// deciding what comes next.
    pub fn reset_to_end(&mut self) {
        self.peeked = None;
        self.lex.set_pos(self.last_end);
    }

    /// Restarts tokenizing at `pos`, discarding any lookahead.
    pub fn seek(&mut self, pos: usize) {
        self.peeked = None;
        self.last_end = pos;
        self.lex.set_pos(pos);
    }

    pub fn error(&self, at: usize, kind: ParseErrorKind) -> ParseError {
        ParseError::new(self.lex.src, at, kind)
    }

    pub fn expect(&mut self, want: &Tok, what: &'static str) -> Result<Token, ParseError> {
        let tok = self.next()?;
        if std::mem::discriminant(&tok.tok) == std::mem::discriminant(want) {
            Ok(tok)
        } else {
            Err(self.error(tok.span.start, ParseErrorKind::Expected(what, tok.tok.to_string())))
        }
    }
}
