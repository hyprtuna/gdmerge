//! A structure-preserving model of a Godot variant literal.
//!
//! The model is deliberately generic: it records the *shape* of a literal
//! (`Name(args...)`, `Array[T]([...])`, `Object(Type, "k": v)`, ...) without
//! knowing Godot's type list, so a future engine type parses without a change
//! here. It is used for comparison only; output always replays the original
//! source text, which is what makes round-tripping lossless.

use std::fmt::Write as _;
use std::ops::Range;

use crate::error::{ParseError, ParseErrorKind};
use crate::lex::{Cursor, Tok};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    Ext,
    Sub,
}

impl RefKind {
    pub fn ctor(self) -> &'static str {
        match self {
            RefKind::Ext => "ExtResource",
            RefKind::Sub => "SubResource",
        }
    }
}

/// An `ExtResource("id")` / `SubResource("id")` occurrence, with the byte range
/// of the quoted id *including* its quotes, relative to the enclosing raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueRef {
    pub kind: RefKind,
    pub id: String,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `true`, `false`, `null`, `nan`, `inf`, `-inf`, and bare type names.
    Ident(String),
    Num(f64),
    Str(String),
    /// `&"..."` StringName.
    Name(String),
    /// `#rrggbb[aa]`, stored without the `#`.
    Color(String),
    Array(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    /// `Name(a, b, ...)`: every constructor, including `ExtResource("1_a")`.
    Call {
        name: String,
        args: Vec<Value>,
    },
    /// `Array[T]([...])`.
    TypedArray {
        ty: Box<Value>,
        items: Vec<Value>,
    },
    /// `Dictionary[K, V]({...})`.
    TypedDict {
        key: Box<Value>,
        val: Box<Value>,
        entries: Vec<(Value, Value)>,
    },
    /// `Object(Type, "prop": value, ...)`.
    Object {
        ty: String,
        props: Vec<(String, Value)>,
    },
}

impl Value {
    /// Renders a canonical, whitespace-independent form used for equality.
    ///
    /// `resolve` maps a resource id to a stable key so that two files which
    /// spell the same resource with different ids still compare equal.
    pub fn canonical(&self, resolve: &mut dyn FnMut(RefKind, &str) -> String) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out, resolve);
        out
    }

    fn write_canonical(&self, out: &mut String, resolve: &mut dyn FnMut(RefKind, &str) -> String) {
        match self {
            Value::Ident(s) => out.push_str(s),
            // `{:?}` gives the shortest representation that round-trips, so the
            // literals `1`, `1.0` and `1.0e0` all canonicalise identically.
            Value::Num(n) => {
                if *n == 0.0 {
                    out.push_str("0.0"); // Collapse -0.0.
                } else {
                    let _ = write!(out, "{n:?}");
                }
            }
            Value::Str(s) => {
                let _ = write!(out, "{s:?}");
            }
            Value::Name(s) => {
                out.push('&');
                let _ = write!(out, "{s:?}");
            }
            Value::Color(s) => {
                out.push('#');
                out.push_str(&s.to_ascii_lowercase());
            }
            Value::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_canonical(out, resolve);
                }
                out.push(']');
            }
            Value::Dict(entries) => {
                out.push('{');
                write_entries(out, entries, resolve);
                out.push('}');
            }
            Value::Call { name, args } => {
                if let (Some(kind), [Value::Str(id)]) = (ref_kind(name), args.as_slice()) {
                    out.push_str(name);
                    out.push('(');
                    let _ = write!(out, "{:?}", resolve(kind, id));
                    out.push(')');
                    return;
                }
                out.push_str(name);
                out.push('(');
                for (i, v) in args.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_canonical(out, resolve);
                }
                out.push(')');
            }
            Value::TypedArray { ty, items } => {
                out.push_str("Array[");
                ty.write_canonical(out, resolve);
                out.push_str("](");
                Value::Array(items.clone()).write_canonical(out, resolve);
                out.push(')');
            }
            Value::TypedDict { key, val, entries } => {
                out.push_str("Dictionary[");
                key.write_canonical(out, resolve);
                out.push(',');
                val.write_canonical(out, resolve);
                out.push_str("]({");
                write_entries(out, entries, resolve);
                out.push_str("})");
            }
            Value::Object { ty, props } => {
                out.push_str("Object(");
                out.push_str(ty);
                for (k, v) in props {
                    let _ = write!(out, ",{k:?}:");
                    v.write_canonical(out, resolve);
                }
                out.push(')');
            }
        }
    }

    /// The string payload of a `"..."` literal, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Name(s) => Some(s),
            _ => None,
        }
    }

    /// The numeric payload, if this is a number.
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
}

fn write_entries(
    out: &mut String,
    entries: &[(Value, Value)],
    resolve: &mut dyn FnMut(RefKind, &str) -> String,
) {
    for (i, (k, v)) in entries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        k.write_canonical(out, resolve);
        out.push(':');
        v.write_canonical(out, resolve);
    }
}

fn ref_kind(name: &str) -> Option<RefKind> {
    match name {
        "ExtResource" => Some(RefKind::Ext),
        "SubResource" => Some(RefKind::Sub),
        _ => None,
    }
}

/// Parses one variant literal. `refs` collects every resource reference with an
/// absolute byte span into the document source.
pub(crate) fn parse_value(
    cur: &mut Cursor<'_>,
    refs: &mut Vec<ValueRef>,
) -> Result<Value, ParseError> {
    let tok = cur.next()?;
    parse_value_from(cur, tok, refs)
}

pub(crate) fn parse_value_from(
    cur: &mut Cursor<'_>,
    tok: crate::lex::Token,
    refs: &mut Vec<ValueRef>,
) -> Result<Value, ParseError> {
    // Every level of nesting funnels through here, so this is the one place the
    // depth has to be counted.
    cur.enter()?;
    let value = parse_value_inner(cur, tok, refs);
    cur.leave();
    value
}

fn parse_value_inner(
    cur: &mut Cursor<'_>,
    tok: crate::lex::Token,
    refs: &mut Vec<ValueRef>,
) -> Result<Value, ParseError> {
    match tok.tok {
        Tok::Num(n) => Ok(Value::Num(n)),
        Tok::Color(c) => Ok(Value::Color(c)),
        Tok::Str { value, name } => Ok(if name { Value::Name(value) } else { Value::Str(value) }),
        Tok::LBracket => parse_array_body(cur, refs).map(Value::Array),
        Tok::LBrace => parse_dict_body(cur, refs).map(Value::Dict),
        Tok::Ident(name) => parse_after_ident(cur, name, refs),
        other => {
            Err(cur.error(tok.span.start, ParseErrorKind::Expected("a value", other.to_string())))
        }
    }
}

fn parse_after_ident(
    cur: &mut Cursor<'_>,
    name: String,
    refs: &mut Vec<ValueRef>,
) -> Result<Value, ParseError> {
    match cur.peek()?.tok {
        Tok::LParen if name == "Object" => parse_object(cur, refs),
        Tok::LParen => {
            cur.next()?;
            let mut args = Vec::new();
            if matches!(cur.peek()?.tok, Tok::RParen) {
                cur.next()?;
                return Ok(Value::Call { name, args });
            }
            // `ExtResource("id")` / `SubResource("id")` are the only id-bearing
            // forms; capture the id token's span so it can be rewritten in place.
            let first = cur.next()?;
            if let (Some(kind), Tok::Str { value, name: false }) = (ref_kind(&name), &first.tok) {
                refs.push(ValueRef { kind, id: value.clone(), span: first.span.clone() });
            }
            args.push(parse_value_from(cur, first, refs)?);
            loop {
                let tok = cur.next()?;
                match tok.tok {
                    Tok::Comma => args.push(parse_value(cur, refs)?),
                    Tok::RParen => return Ok(Value::Call { name, args }),
                    other => {
                        return Err(cur.error(
                            tok.span.start,
                            ParseErrorKind::Expected("',' or ')'", other.to_string()),
                        ))
                    }
                }
            }
        }
        Tok::LBracket if name == "Array" => parse_typed_array(cur, refs),
        Tok::LBracket if name == "Dictionary" => parse_typed_dict(cur, refs),
        _ => Ok(Value::Ident(name)),
    }
}

fn parse_array_body(
    cur: &mut Cursor<'_>,
    refs: &mut Vec<ValueRef>,
) -> Result<Vec<Value>, ParseError> {
    let mut items = Vec::new();
    loop {
        if matches!(cur.peek()?.tok, Tok::RBracket) {
            cur.next()?;
            return Ok(items);
        }
        items.push(parse_value(cur, refs)?);
        let tok = cur.next()?;
        match tok.tok {
            Tok::Comma => {}
            Tok::RBracket => return Ok(items),
            other => {
                return Err(cur.error(
                    tok.span.start,
                    ParseErrorKind::Expected("',' or ']'", other.to_string()),
                ))
            }
        }
    }
}

fn parse_dict_body(
    cur: &mut Cursor<'_>,
    refs: &mut Vec<ValueRef>,
) -> Result<Vec<(Value, Value)>, ParseError> {
    let mut entries = Vec::new();
    loop {
        if matches!(cur.peek()?.tok, Tok::RBrace) {
            cur.next()?;
            return Ok(entries);
        }
        let key = parse_value(cur, refs)?;
        cur.expect(&Tok::Colon, "':'")?;
        let val = parse_value(cur, refs)?;
        entries.push((key, val));
        let tok = cur.next()?;
        match tok.tok {
            Tok::Comma => {}
            Tok::RBrace => return Ok(entries),
            other => {
                return Err(cur.error(
                    tok.span.start,
                    ParseErrorKind::Expected("',' or '}'", other.to_string()),
                ))
            }
        }
    }
}

/// Parses the `T` in `Array[T]` / `Dictionary[K, V]`, which is an identifier
/// that may itself be a resource reference such as `ExtResource("1_a")`.
fn parse_type_slot(cur: &mut Cursor<'_>, refs: &mut Vec<ValueRef>) -> Result<Value, ParseError> {
    let tok = cur.next()?;
    let Tok::Ident(name) = tok.tok else {
        return Err(cur.error(
            tok.span.start,
            ParseErrorKind::Expected("a type identifier", tok.tok.to_string()),
        ));
    };
    if matches!(cur.peek()?.tok, Tok::LParen) {
        return parse_after_ident(cur, name, refs);
    }
    Ok(Value::Ident(name))
}

fn parse_typed_array(cur: &mut Cursor<'_>, refs: &mut Vec<ValueRef>) -> Result<Value, ParseError> {
    cur.expect(&Tok::LBracket, "'['")?;
    let ty = parse_type_slot(cur, refs)?;
    cur.expect(&Tok::RBracket, "']'")?;
    cur.expect(&Tok::LParen, "'('")?;
    cur.expect(&Tok::LBracket, "'['")?;
    let items = parse_array_body(cur, refs)?;
    cur.expect(&Tok::RParen, "')'")?;
    Ok(Value::TypedArray { ty: Box::new(ty), items })
}

fn parse_typed_dict(cur: &mut Cursor<'_>, refs: &mut Vec<ValueRef>) -> Result<Value, ParseError> {
    cur.expect(&Tok::LBracket, "'['")?;
    let key = parse_type_slot(cur, refs)?;
    cur.expect(&Tok::Comma, "','")?;
    let val = parse_type_slot(cur, refs)?;
    cur.expect(&Tok::RBracket, "']'")?;
    cur.expect(&Tok::LParen, "'('")?;
    cur.expect(&Tok::LBrace, "'{'")?;
    let entries = parse_dict_body(cur, refs)?;
    cur.expect(&Tok::RParen, "')'")?;
    Ok(Value::TypedDict { key: Box::new(key), val: Box::new(val), entries })
}

fn parse_object(cur: &mut Cursor<'_>, refs: &mut Vec<ValueRef>) -> Result<Value, ParseError> {
    cur.expect(&Tok::LParen, "'('")?;
    let tok = cur.next()?;
    let Tok::Ident(ty) = tok.tok else {
        return Err(cur.error(
            tok.span.start,
            ParseErrorKind::Expected("an object type", tok.tok.to_string()),
        ));
    };
    let mut props = Vec::new();
    loop {
        let tok = cur.next()?;
        match tok.tok {
            Tok::RParen => return Ok(Value::Object { ty, props }),
            Tok::Comma => {}
            other => {
                return Err(cur.error(
                    tok.span.start,
                    ParseErrorKind::Expected("',' or ')'", other.to_string()),
                ))
            }
        }
        let tok = cur.next()?;
        match tok.tok {
            Tok::RParen => return Ok(Value::Object { ty, props }),
            Tok::Str { value, .. } => {
                cur.expect(&Tok::Colon, "':'")?;
                props.push((value, parse_value(cur, refs)?));
            }
            other => {
                return Err(cur.error(
                    tok.span.start,
                    ParseErrorKind::Expected("a property name", other.to_string()),
                ))
            }
        }
    }
}
