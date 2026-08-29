//! The lossless document model: every byte of the source is stored in exactly
//! one field, so `Document::to_source` reproduces the input verbatim.

use std::fmt::Write as _;

use crate::error::{ParseError, ParseErrorKind};
use crate::lex::{Cursor, Tok};
use crate::value::{parse_value, RefKind, Value, ValueRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    GdScene,
    GdResource,
    ExtResource,
    SubResource,
    Node,
    Connection,
    Editable,
    /// The `[resource]` section of a `.tres` file.
    Resource,
}

impl SectionKind {
    fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "gd_scene" => SectionKind::GdScene,
            "gd_resource" => SectionKind::GdResource,
            "ext_resource" => SectionKind::ExtResource,
            "sub_resource" => SectionKind::SubResource,
            "node" => SectionKind::Node,
            "connection" => SectionKind::Connection,
            "editable" => SectionKind::Editable,
            "resource" => SectionKind::Resource,
            _ => return None,
        })
    }

    /// Whether a header field carries no meaning of its own: `load_steps` is
    /// recomputed from the resource counts, and a resource's `id` is randomised
    /// per file. Neither is a difference worth reporting or merging.
    pub fn is_derived_field(self, name: &str) -> bool {
        match self {
            SectionKind::GdScene | SectionKind::GdResource => name == "load_steps",
            SectionKind::ExtResource | SectionKind::SubResource => name == "id",
            _ => false,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            SectionKind::GdScene => "gd_scene",
            SectionKind::GdResource => "gd_resource",
            SectionKind::ExtResource => "ext_resource",
            SectionKind::SubResource => "sub_resource",
            SectionKind::Node => "node",
            SectionKind::Connection => "connection",
            SectionKind::Editable => "editable",
            SectionKind::Resource => "resource",
        }
    }
}

/// A `name=value` pair inside a section header.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Source text between the previous header item and this field's name.
    pub sep_before: String,
    pub name: String,
    /// Source text between the name and `=`.
    pub sep_eq: String,
    /// Source text between `=` and the value.
    pub sep_val: String,
    pub value: Value,
    /// Exact source text of the value.
    pub value_raw: String,
    /// Resource references inside `value_raw`, with spans relative to it.
    pub refs: Vec<ValueRef>,
}

/// A `key = value` assignment following a section header.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Whitespace and `;` comments preceding the key.
    pub lead: String,
    /// Exact source text of the key, including quotes when it is quoted.
    pub key_raw: String,
    /// Decoded key, as Godot's loader sees it.
    pub key: String,
    /// Source text between the key and `=`.
    pub sep_eq: String,
    /// Source text between `=` and the value.
    pub sep_val: String,
    pub value: Value,
    pub value_raw: String,
    /// Resource references inside `value_raw`, with spans relative to it.
    pub refs: Vec<ValueRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub kind: SectionKind,
    pub tag: String,
    /// Source text between `[` and the tag name.
    pub open_sep: String,
    pub fields: Vec<Field>,
    /// Source text between the last header item and `]`.
    pub close_sep: String,
    pub props: Vec<Property>,
    /// Everything after the last property up to the next section or EOF.
    pub trailing: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Whitespace and comments preceding the first section.
    pub lead: String,
    pub sections: Vec<Section>,
}

impl Field {
    /// The field value with its resource ids rewritten by `remap`.
    pub fn rendered(&self, remap: &dyn Fn(RefKind, &str) -> Option<String>) -> String {
        splice_refs(&self.value_raw, &self.refs, remap)
    }
}

impl Property {
    /// The property value with its resource ids rewritten by `remap`.
    pub fn rendered(&self, remap: &dyn Fn(RefKind, &str) -> Option<String>) -> String {
        splice_refs(&self.value_raw, &self.refs, remap)
    }
}

fn splice_refs(
    raw: &str,
    refs: &[ValueRef],
    remap: &dyn Fn(RefKind, &str) -> Option<String>,
) -> String {
    if refs.is_empty() {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut cursor = 0usize;
    for r in refs {
        let Some(new_id) = remap(r.kind, &r.id) else { continue };
        if new_id == r.id {
            continue;
        }
        out.push_str(&raw[cursor..r.span.start]);
        let _ = write!(out, "\"{new_id}\"");
        cursor = r.span.end;
    }
    out.push_str(&raw[cursor..]);
    out
}

impl Section {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn field_str(&self, name: &str) -> Option<&str> {
        self.field(name).and_then(|f| f.value.as_str())
    }

    pub fn prop(&self, key: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.key == key)
    }

    /// Every resource reference in this section's header fields and properties.
    pub fn refs(&self) -> impl Iterator<Item = &ValueRef> {
        self.fields
            .iter()
            .flat_map(|f| f.refs.iter())
            .chain(self.props.iter().flat_map(|p| p.refs.iter()))
    }

    /// Renders the section back to source, rewriting resource ids via `remap`.
    pub fn render(&self, remap: &dyn Fn(RefKind, &str) -> Option<String>, out: &mut String) {
        out.push('[');
        out.push_str(&self.open_sep);
        out.push_str(&self.tag);
        for f in &self.fields {
            out.push_str(&f.sep_before);
            out.push_str(&f.name);
            out.push_str(&f.sep_eq);
            out.push('=');
            out.push_str(&f.sep_val);
            out.push_str(&f.rendered(remap));
        }
        out.push_str(&self.close_sep);
        out.push(']');
        for p in &self.props {
            out.push_str(&p.lead);
            out.push_str(&p.key_raw);
            out.push_str(&p.sep_eq);
            out.push('=');
            out.push_str(&p.sep_val);
            out.push_str(&p.rendered(remap));
        }
        out.push_str(&self.trailing);
    }
}

impl Document {
    pub fn parse(src: &str) -> Result<Document, ParseError> {
        parse_document(src)
    }

    /// Reproduces the source text. For an unmodified document this is byte-exact.
    pub fn to_source(&self) -> String {
        self.render(&|_, _| None)
    }

    pub fn render(&self, remap: &dyn Fn(RefKind, &str) -> Option<String>) -> String {
        let mut out = String::new();
        out.push_str(&self.lead);
        for s in &self.sections {
            s.render(remap, &mut out);
        }
        out
    }

    pub fn is_scene(&self) -> bool {
        matches!(self.sections.first().map(|s| s.kind), Some(SectionKind::GdScene))
    }

    pub fn header(&self) -> &Section {
        self.sections.first().expect("a parsed document always has a header section")
    }

    pub fn sections_of(&self, kind: SectionKind) -> impl Iterator<Item = &Section> {
        self.sections.iter().filter(move |s| s.kind == kind)
    }
}

fn parse_document(src: &str) -> Result<Document, ParseError> {
    let bytes = src.as_bytes();
    let mut cur = Cursor::new(src);
    let mut doc = Document { lead: String::new(), sections: Vec::new() };
    let mut consumed = 0usize;

    loop {
        cur.seek(consumed);
        let at = cur.skip_trivia_pos();
        let gap = &src[consumed..at];
        if at >= src.len() {
            match doc.sections.last_mut() {
                Some(s) => s.trailing.push_str(gap),
                None => doc.lead.push_str(gap),
            }
            break;
        }
        if bytes[at] == b'[' {
            match doc.sections.last_mut() {
                Some(s) => s.trailing.push_str(gap),
                None => doc.lead.push_str(gap),
            }
            cur.seek(at);
            let section = parse_section(&mut cur)?;
            doc.sections.push(section);
            consumed = cur.end();
        } else {
            let Some(section) = doc.sections.last_mut() else {
                return Err(cur.error(at, ParseErrorKind::PropertyOutsideSection));
            };
            cur.seek(at);
            let mut prop = parse_property(&mut cur)?;
            prop.lead = gap.to_string();
            section.props.push(prop);
            consumed = cur.end();
        }
    }

    if doc.sections.is_empty()
        || !matches!(doc.sections[0].kind, SectionKind::GdScene | SectionKind::GdResource)
    {
        return Err(ParseError::new(src, 0, ParseErrorKind::NotAGodotTextResource));
    }
    Ok(doc)
}

fn parse_section(cur: &mut Cursor<'_>) -> Result<Section, ParseError> {
    let src = cur.src();
    let open = cur.expect(&Tok::LBracket, "'['")?;
    let name_tok = cur.next()?;
    let Tok::Ident(mut tag) = name_tok.tok else {
        return Err(cur.error(
            name_tok.span.start,
            ParseErrorKind::Expected("a section name", name_tok.tok.to_string()),
        ));
    };
    let open_sep = src[open.span.end..name_tok.span.start].to_string();

    // Godot allows suffixed tags such as `[some_prop.Android]`.
    loop {
        cur.reset_to_end();
        match cur.peek()?.tok {
            Tok::Period => tag.push('.'),
            Tok::Colon => tag.push(':'),
            _ => break,
        }
        cur.next()?;
        let part = cur.next()?;
        let Tok::Ident(id) = part.tok else {
            return Err(cur.error(
                part.span.start,
                ParseErrorKind::Expected("an identifier", part.tok.to_string()),
            ));
        };
        tag.push_str(&id);
    }

    let Some(kind) = SectionKind::from_tag(&tag) else {
        return Err(cur.error(name_tok.span.start, ParseErrorKind::UnknownSection(tag)));
    };

    let mut fields = Vec::new();
    let close_sep;
    loop {
        cur.reset_to_end();
        let item_start = cur.end();
        let tok = cur.next()?;
        match tok.tok {
            Tok::RBracket => {
                close_sep = src[item_start..tok.span.start].to_string();
                break;
            }
            Tok::Ident(name) => {
                let sep_before = src[item_start..tok.span.start].to_string();
                let eq = cur.expect(&Tok::Equal, "'=' after a field name")?;
                let sep_eq = src[tok.span.end..eq.span.start].to_string();
                let value_start = cur.next_start()?;
                let sep_val = src[eq.span.end..value_start].to_string();
                let mut refs = Vec::new();
                let value = parse_value(cur, &mut refs)?;
                cur.reset_to_end();
                let value_raw = src[value_start..cur.end()].to_string();
                rebase_refs(&mut refs, value_start);
                fields.push(Field { sep_before, name, sep_eq, sep_val, value, value_raw, refs });
            }
            other => {
                return Err(cur.error(
                    tok.span.start,
                    ParseErrorKind::Expected("a field name or ']'", other.to_string()),
                ))
            }
        }
    }

    Ok(Section {
        kind,
        tag,
        open_sep,
        fields,
        close_sep,
        props: Vec::new(),
        trailing: String::new(),
    })
}

fn parse_property(cur: &mut Cursor<'_>) -> Result<Property, ParseError> {
    let src = cur.src();
    let bytes = src.as_bytes();
    let start = cur.end();

    // A quoted key may itself contain '=', so it has to be tokenized.
    let (key, key_end) = if bytes[start] == b'"' {
        let tok = cur.next()?;
        let Tok::Str { value, .. } = tok.tok else {
            return Err(
                cur.error(start, ParseErrorKind::Expected("a property name", "a value".into()))
            );
        };
        (value, tok.span.end)
    } else {
        let mut i = start;
        while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b'\n' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'\n' {
            return Err(cur.error(start, ParseErrorKind::TrailingGarbage));
        }
        // Godot drops every whitespace byte inside an unquoted property name.
        let raw = &src[start..i];
        let key: String = raw.chars().filter(|c| (*c as u32) > 32).collect();
        (key, start + raw.trim_end().len())
    };

    let mut eq = key_end;
    while eq < bytes.len() && bytes[eq] != b'=' {
        eq += 1;
    }
    if eq >= bytes.len() {
        return Err(cur.error(start, ParseErrorKind::TrailingGarbage));
    }

    let key_raw = src[start..key_end].to_string();
    let sep_eq = src[key_end..eq].to_string();
    cur.seek(eq + 1);
    let value_start = cur.next_start()?;
    let sep_val = src[eq + 1..value_start].to_string();
    let mut refs = Vec::new();
    let value = parse_value(cur, &mut refs)?;
    cur.reset_to_end();
    let value_raw = src[value_start..cur.end()].to_string();
    rebase_refs(&mut refs, value_start);

    Ok(Property { lead: String::new(), key_raw, key, sep_eq, sep_val, value, value_raw, refs })
}

fn rebase_refs(refs: &mut [ValueRef], base: usize) {
    for r in refs {
        r.span = (r.span.start - base)..(r.span.end - base);
    }
}
