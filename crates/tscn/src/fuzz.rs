//! Entry points for fuzz targets, behind the `fuzzing` feature.
//!
//! The lexer and the value parser are internals, but they are also the two
//! places that read untrusted bytes, so the fuzzer needs to reach them
//! directly. Nothing here is part of the public API and none of it is subject
//! to the crate's usual compatibility promise.

use crate::lex::{Cursor, Lexer, Tok};

/// Runs the tokenizer over `src` to end of input or to the first error.
///
/// The contract being fuzzed is that no input, valid or not, makes it panic,
/// loop forever, or read outside the string.
pub fn tokenize(src: &str) {
    let mut lexer = Lexer::new(src);
    let mut budget = src.len() + 1;
    loop {
        match lexer.next_token() {
            Ok(token) => {
                if token.tok == Tok::Eof {
                    return;
                }
                assert!(
                    token.span.start <= token.span.end && token.span.end <= src.len(),
                    "token span {:?} is not inside a {} byte input",
                    token.span,
                    src.len()
                );
                assert!(src.is_char_boundary(token.span.start));
                assert!(src.is_char_boundary(token.span.end));
            }
            Err(_) => return,
        }
        // Every token has to consume at least one byte, so this many iterations
        // is only reachable if the tokenizer stopped making progress.
        budget = budget.saturating_sub(1);
        assert!(budget > 0, "the tokenizer stopped advancing");
    }
}

/// Parses one variant literal from `src`.
pub fn parse_value(src: &str) {
    let mut cursor = Cursor::new(src);
    let mut refs = crate::value::Pointers::default();
    if let Ok(value) = crate::value::parse_value(&mut cursor, &mut refs) {
        // Canonicalising must be total: it is used to compare every value in
        // every merge, so a value that parses but cannot be canonicalised would
        // take down a merge rather than a parse.
        let _ = value.canonical(&mut |_, id| id.to_string());
        for r in &refs.refs {
            assert!(r.span.end <= src.len(), "reference span outside the input");
        }
        for p in &refs.paths {
            assert!(p.span.end <= src.len(), "node path span outside the input");
        }
    }
}

/// Parses a whole document and checks the invariant everything else rests on:
/// a document that parses re-serialises to exactly the bytes it came from.
pub fn parse_document(src: &str) {
    if let Ok(doc) = crate::Document::parse(src) {
        assert_eq!(doc.to_source(), src, "round trip lost or changed bytes");
    }
}
