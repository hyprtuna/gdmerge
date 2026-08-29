//! Parse errors carrying a line and column into the offending source.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    ExpectedQuoteAfterAmpersand,
    UnterminatedString,
    UnpairedSurrogate,
    MalformedHexEscape,
    MalformedNumber,
    UnexpectedCharacter(char),
    Expected(&'static str, String),
    UnknownSection(String),
    MissingField(&'static str, &'static str),
    PropertyOutsideSection,
    NotAGodotTextResource,
    TrailingGarbage,
    ValueTooDeep(usize),
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseErrorKind::ExpectedQuoteAfterAmpersand => write!(f, "expected '\"' after '&'"),
            ParseErrorKind::UnterminatedString => write!(f, "unterminated string"),
            ParseErrorKind::UnpairedSurrogate => {
                write!(f, "invalid UTF-16 escape sequence: unpaired surrogate")
            }
            ParseErrorKind::MalformedHexEscape => write!(f, "malformed hex escape in string"),
            ParseErrorKind::MalformedNumber => write!(f, "malformed number literal"),
            ParseErrorKind::UnexpectedCharacter(c) => write!(f, "unexpected character {c:?}"),
            ParseErrorKind::Expected(what, got) => write!(f, "expected {what}, found {got}"),
            ParseErrorKind::UnknownSection(tag) => write!(f, "unknown section tag [{tag}]"),
            ParseErrorKind::MissingField(tag, field) => {
                write!(f, "[{tag}] is missing the required '{field}' field")
            }
            ParseErrorKind::PropertyOutsideSection => {
                write!(f, "property assignment before any section header")
            }
            ParseErrorKind::NotAGodotTextResource => {
                write!(f, "file does not start with [gd_scene] or [gd_resource]")
            }
            ParseErrorKind::TrailingGarbage => write!(f, "unexpected trailing content"),
            ParseErrorKind::ValueTooDeep(limit) => {
                write!(f, "value nested more than {limit} levels deep")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub kind: ParseErrorKind,
}

impl ParseError {
    pub(crate) fn new(src: &str, offset: usize, kind: ParseErrorKind) -> Self {
        let offset = offset.min(src.len());
        let before = &src[..offset];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before.rfind('\n').map_or(offset, |nl| offset - nl - 1) + 1;
        ParseError { line, column, kind }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.kind)
    }
}

impl std::error::Error for ParseError {}
