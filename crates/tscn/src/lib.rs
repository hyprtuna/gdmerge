//! Lossless parsing, semantic modelling and 3-way merging of Godot 4 text
//! scene and resource files (`.tscn` / `.tres`).
//!
//! The grammar mirrors Godot's own `VariantParser` and `ResourceLoaderText`.
//! Parsing keeps every byte of the input, so [`Document::to_source`] reproduces
//! the original file exactly; the semantic layer on top is what diffing and
//! merging reason about.
//!
//! ```
//! let src = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n";
//! let doc = tscn::Document::parse(src).unwrap();
//! assert_eq!(doc.to_source(), src);
//! assert!(doc.is_scene());
//! ```

mod doc;
mod error;
mod lex;
mod value;

pub use doc::{Document, Field, Property, Section, SectionKind};
pub use error::{ParseError, ParseErrorKind};
pub use value::{RefKind, Value, ValueRef};
