//! Semantic identity for the entities in a document.
//!
//! Godot mints resource ids per file and randomises them (`1_abcde`,
//! `CircleShape2D_abcde`), so the same logical resource carries different ids on
//! two branches. Every comparison therefore runs against *identity keys* rather
//! than ids: external resources key on their `uid` (falling back to `path`),
//! sub-resources on their type plus their fully resolved content, and nodes on
//! their full scene-tree path.

use std::collections::HashMap;

use crate::doc::{Document, Section, SectionKind};
use crate::value::RefKind;

/// A stable, id-independent identity for one section of a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityId {
    /// The `[gd_scene]` / `[gd_resource]` header.
    Header,
    /// Keyed by `uid`, or by `path` when the file records no uid.
    Ext(String),
    /// Keyed by type and fully resolved content.
    Sub(String),
    /// Keyed by full node path; the root node is `.`.
    Node(String),
    /// Keyed by `from`, `to`, `signal`, `method`.
    Connection(String, String, String, String),
    Editable(String),
    /// The `[resource]` section of a `.tres`.
    Resource,
}

impl EntityId {
    pub fn describe(&self) -> String {
        match self {
            EntityId::Header => "header".to_string(),
            EntityId::Ext(k) => format!("ext_resource {k}"),
            EntityId::Sub(k) => format!("sub_resource {}", short(k)),
            EntityId::Node(p) if p == "." => "root node".to_string(),
            EntityId::Node(p) => format!("node {p}"),
            EntityId::Connection(from, to, sig, m) => {
                format!("connection {from}.{sig} -> {to}.{m}()")
            }
            EntityId::Editable(p) => format!("editable {p}"),
            EntityId::Resource => "resource".to_string(),
        }
    }

    /// Sort rank matching the order Godot writes sections in.
    pub fn rank(&self) -> u8 {
        match self {
            EntityId::Header => 0,
            EntityId::Ext(_) => 1,
            EntityId::Sub(_) => 2,
            EntityId::Resource => 3,
            EntityId::Node(_) => 4,
            EntityId::Connection(..) => 5,
            EntityId::Editable(_) => 6,
        }
    }
}

/// A sub-resource key is its whole canonical content, which is unreadable. Show
/// the resource type plus a short digest so two of the same type stay distinct.
fn short(key: &str) -> String {
    let ty = key
        .split("#type=")
        .nth(1)
        .and_then(|rest| rest.split(['#', '\n']).next())
        .map(|t| t.trim_matches('"'))
        .filter(|t| !t.is_empty())
        .unwrap_or("resource");
    format!("{ty} ({})", &hash_hex(key)[..8])
}

fn hash_hex(s: &str) -> String {
    // FNV-1a, purely for a short human-facing label.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// A document viewed through its semantic identities.
pub struct Scene<'d> {
    pub doc: &'d Document,
    /// Identity of each section, parallel to `doc.sections`.
    ids: Vec<EntityId>,
    by_id: HashMap<EntityId, usize>,
    ext_key: HashMap<String, String>,
    sub_key: HashMap<String, String>,
}

impl<'d> Scene<'d> {
    pub fn new(doc: &'d Document) -> Scene<'d> {
        let ext_key = build_ext_keys(doc);
        let sub_key = build_sub_keys(doc, &ext_key);

        let mut ids = Vec::with_capacity(doc.sections.len());
        let mut by_id = HashMap::new();
        for (i, section) in doc.sections.iter().enumerate() {
            let id = identity(section, &ext_key, &sub_key);
            // A duplicate key keeps the first occurrence; `check` reports the clash.
            by_id.entry(id.clone()).or_insert(i);
            ids.push(id);
        }
        Scene { doc, ids, by_id, ext_key, sub_key }
    }

    pub fn ids(&self) -> &[EntityId] {
        &self.ids
    }

    pub fn id_of(&self, index: usize) -> &EntityId {
        &self.ids[index]
    }

    /// A human-readable label for an entity, enriched with detail that only the
    /// document carries: a root node's name, for instance, which its identity
    /// (always `.`) deliberately leaves out so renaming the root is a property
    /// change rather than a delete and an add.
    pub fn describe(&self, id: &EntityId) -> String {
        match (id, self.section(id)) {
            (EntityId::Node(p), Some(s)) if p == "." => match s.field_str("name") {
                Some(name) => format!("root node \"{name}\""),
                None => id.describe(),
            },
            _ => id.describe(),
        }
    }

    pub fn section(&self, id: &EntityId) -> Option<&'d Section> {
        self.by_id.get(id).map(|i| &self.doc.sections[*i])
    }

    pub fn index_of(&self, id: &EntityId) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// Identity keys in file order, so ordering changes are observable.
    pub fn order(&self) -> Vec<EntityId> {
        self.ids.clone()
    }

    /// Maps a local resource id to its identity key.
    pub fn key_for(&self, kind: RefKind, id: &str) -> Option<&str> {
        match kind {
            RefKind::Ext => self.ext_key.get(id).map(String::as_str),
            RefKind::Sub => self.sub_key.get(id).map(String::as_str),
        }
    }

    /// The canonical form of a section, used for equality between branches.
    pub fn canonical(&self, index: usize) -> String {
        canonical_section(&self.doc.sections[index], &self.ext_key, &self.sub_key)
    }

    /// Canonical form of one header field, ids resolved to identity keys.
    pub fn canonical_field(&self, section: &Section, name: &str) -> Option<String> {
        let f = section.field(name)?;
        Some(f.value.canonical(&mut |kind, id| self.resolve(kind, id)))
    }

    /// Canonical form of one property, ids resolved to identity keys.
    pub fn canonical_prop(&self, section: &Section, key: &str) -> Option<String> {
        let p = section.prop(key)?;
        Some(p.value.canonical(&mut |kind, id| self.resolve(kind, id)))
    }

    fn resolve(&self, kind: RefKind, id: &str) -> String {
        resolve_ref(&self.ext_key, &self.sub_key, kind, id)
    }
}

fn resolve_ref(
    ext_key: &HashMap<String, String>,
    sub_key: &HashMap<String, String>,
    kind: RefKind,
    id: &str,
) -> String {
    let table = match kind {
        RefKind::Ext => ext_key,
        RefKind::Sub => sub_key,
    };
    // An id with no matching declaration is dangling; keep it verbatim so the
    // difference still shows up rather than silently comparing equal.
    table.get(id).cloned().unwrap_or_else(|| format!("?{id}"))
}

fn build_ext_keys(doc: &Document) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for s in doc.sections_of(SectionKind::ExtResource) {
        let Some(id) = s.field_str("id") else { continue };
        let key = s.field_str("uid").or_else(|| s.field_str("path")).unwrap_or(id).to_string();
        map.insert(id.to_string(), key);
    }
    map
}

/// Content-keys every sub-resource. Sub-resources may reference each other, so
/// keys are resolved by fixed point: each pass keys the sub-resources whose
/// dependencies are already known, and any residual cycle falls back to a key
/// built from the id itself.
fn build_sub_keys(doc: &Document, ext_key: &HashMap<String, String>) -> HashMap<String, String> {
    let subs: Vec<&Section> = doc.sections_of(SectionKind::SubResource).collect();
    let mut keys: HashMap<String, String> = HashMap::new();
    for _ in 0..subs.len() + 1 {
        let mut changed = false;
        for s in &subs {
            let Some(id) = s.field_str("id") else { continue };
            if keys.contains_key(id) {
                continue;
            }
            let deps_ready = s
                .refs()
                .filter(|r| r.kind == RefKind::Sub)
                .all(|r| r.id == id || keys.contains_key(&r.id));
            if !deps_ready {
                continue;
            }
            let key = canonical_section(s, ext_key, &keys);
            keys.insert(id.to_string(), key);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    // Anything left is part of a reference cycle.
    for s in &subs {
        if let Some(id) = s.field_str("id") {
            keys.entry(id.to_string()).or_insert_with(|| format!("cycle:{id}"));
        }
    }
    // Distinct sub-resources can legitimately have identical content; keep them
    // apart by suffixing repeats in declaration order.
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = HashMap::new();
    for s in &subs {
        let Some(id) = s.field_str("id") else { continue };
        let base = keys.get(id).cloned().unwrap_or_else(|| format!("?{id}"));
        let n = seen.entry(base.clone()).or_insert(0);
        let key = if *n == 0 { base } else { format!("{base}~{n}") };
        *n += 1;
        out.insert(id.to_string(), key);
    }
    out
}

/// Canonical text for a section, excluding the fields that carry no meaning of
/// their own: a randomised per-file `id`, and the derived `load_steps`.
fn canonical_section(
    s: &Section,
    ext_key: &HashMap<String, String>,
    sub_key: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str(&s.tag);
    let mut fields: Vec<_> =
        s.fields.iter().filter(|f| !s.kind.is_derived_field(&f.name)).collect();
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    for f in fields {
        out.push('#');
        out.push_str(&f.name);
        out.push('=');
        out.push_str(&f.value.canonical(&mut |k, i| resolve_ref(ext_key, sub_key, k, i)));
    }
    for p in &s.props {
        out.push('\n');
        out.push_str(&p.key);
        out.push('=');
        out.push_str(&p.value.canonical(&mut |k, i| resolve_ref(ext_key, sub_key, k, i)));
    }
    out
}

fn identity(
    s: &Section,
    ext_key: &HashMap<String, String>,
    sub_key: &HashMap<String, String>,
) -> EntityId {
    match s.kind {
        SectionKind::GdScene | SectionKind::GdResource => EntityId::Header,
        SectionKind::Resource => EntityId::Resource,
        SectionKind::ExtResource => EntityId::Ext(
            s.field_str("uid")
                .or_else(|| s.field_str("path"))
                .or_else(|| s.field_str("id"))
                .unwrap_or_default()
                .to_string(),
        ),
        SectionKind::SubResource => EntityId::Sub(
            s.field_str("id")
                .and_then(|id| sub_key.get(id).cloned())
                .unwrap_or_else(|| canonical_section(s, ext_key, sub_key)),
        ),
        SectionKind::Node => EntityId::Node(node_path(s)),
        SectionKind::Connection => EntityId::Connection(
            s.field_str("from").unwrap_or_default().to_string(),
            s.field_str("to").unwrap_or_default().to_string(),
            s.field_str("signal").unwrap_or_default().to_string(),
            s.field_str("method").unwrap_or_default().to_string(),
        ),
        SectionKind::Editable => {
            EntityId::Editable(s.field_str("path").unwrap_or_default().to_string())
        }
    }
}

/// The full scene-tree path of a `[node]` section. The root node is `.`.
pub fn node_path(s: &Section) -> String {
    let name = s.field_str("name").unwrap_or_default();
    match s.field_str("parent") {
        None => ".".to_string(),
        Some(".") => name.to_string(),
        Some(parent) => format!("{parent}/{name}"),
    }
}
