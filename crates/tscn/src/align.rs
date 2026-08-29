//! Matching nodes across versions of a document when their path changed.
//!
//! A node's identity is its scene-tree path, so renaming or reparenting one
//! looks, on its own, like a delete plus an add. Pairing the two back up is what
//! lets a rename on one branch merge with an edit on the other, and it is the
//! same pairing the semantic diff reports as a move.
//!
//! The pairing is deliberately conservative: two nodes match only when
//! everything about them except their name, parent and sibling index is
//! identical. A branch that renames *and* edits in one step is therefore not
//! matched, and falls back to a delete against a modify.

use std::collections::{HashMap, HashSet};

use crate::scene::{EntityId, Scene};

/// Which of the three inputs a lookup refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Which {
    Base,
    Ours,
    Theirs,
}

/// Node paths that moved between two documents, as `(from, to)` pairs.
pub(crate) fn node_moves(from: &Scene<'_>, to: &Scene<'_>) -> Vec<(String, String)> {
    let from_ids: HashSet<&EntityId> = from.ids().iter().collect();
    let to_ids: HashSet<&EntityId> = to.ids().iter().collect();

    let mut gone: Vec<&EntityId> =
        from.ids().iter().filter(|id| is_node(id) && !to_ids.contains(id)).collect();
    let mut fresh: Vec<&EntityId> =
        to.ids().iter().filter(|id| is_node(id) && !from_ids.contains(id)).collect();
    gone.sort();
    fresh.sort();

    let mut by_content: HashMap<String, Vec<&EntityId>> = HashMap::new();
    for id in &fresh {
        let Some(i) = to.index_of(id) else { continue };
        by_content.entry(content_key(to, i)).or_default().push(id);
    }

    let mut taken: HashSet<&EntityId> = HashSet::new();
    let mut pairs = Vec::new();
    for old in gone {
        let Some(i) = from.index_of(old) else { continue };
        let Some(candidates) = by_content.get(&content_key(from, i)) else { continue };
        let Some(old_section) = from.section(old) else { continue };
        let old_name = old_section.field_str("name").unwrap_or_default();
        let old_parent = old_section.field_str("parent").unwrap_or_default();

        // Several nodes can share contents. Prefer the reading that changes the
        // least: same name means a reparent, same parent means a rename.
        let best = candidates
            .iter()
            .filter(|c| !taken.contains(**c))
            .max_by_key(|c| {
                let Some(s) = to.section(c) else { return 0 };
                let same_name = s.field_str("name").unwrap_or_default() == old_name;
                let same_parent = s.field_str("parent").unwrap_or_default() == old_parent;
                usize::from(same_name) * 2 + usize::from(same_parent)
            })
            .copied();
        if let Some(to_id) = best {
            taken.insert(to_id);
            if let (EntityId::Node(a), EntityId::Node(b)) = (old, to_id) {
                pairs.push((a.clone(), b.clone()));
            }
        }
    }
    pairs
}

fn is_node(id: &EntityId) -> bool {
    matches!(id, EntityId::Node(_))
}

/// Canonical content of a node ignoring the fields that define its identity.
pub(crate) fn content_key(scene: &Scene<'_>, index: usize) -> String {
    let s = &scene.doc.sections[index];
    let mut out = String::new();
    let mut fields: Vec<_> = s
        .fields
        .iter()
        .filter(|f| f.name != "name" && f.name != "parent" && f.name != "index")
        .collect();
    fields.sort_by(|x, y| x.name.cmp(&y.name));
    for f in fields {
        out.push('#');
        out.push_str(&f.name);
        out.push('=');
        out.push_str(scene.canonical_field(s, &f.name).unwrap_or_default().as_str());
    }
    for p in &s.props {
        out.push('\n');
        out.push_str(&p.key);
        out.push('=');
        out.push_str(scene.canonical_prop(s, &p.key).unwrap_or_default().as_str());
    }
    out
}

/// Puts the three inputs of a merge into one shared naming.
///
/// Each side gets a table rewriting the node paths it uses into the paths the
/// merged document will use, so an entity that one branch renamed is recognised
/// as the same entity the other branch edited.
pub(crate) struct Alignment {
    paths: HashMap<Which, HashMap<String, String>>,
    /// Base paths that both branches renamed, to different places.
    contested: HashSet<String>,
}

impl Alignment {
    pub(crate) fn new(base: &Scene<'_>, ours: &Scene<'_>, theirs: &Scene<'_>) -> Alignment {
        let to_ours: HashMap<String, String> = node_moves(base, ours).into_iter().collect();
        let to_theirs: HashMap<String, String> = node_moves(base, theirs).into_iter().collect();

        let mut base_map = HashMap::new();
        let mut ours_map = HashMap::new();
        let mut theirs_map = HashMap::new();
        let mut contested = HashSet::new();

        for id in base.ids() {
            let EntityId::Node(b) = id else { continue };
            let o = to_ours.get(b);
            let t = to_theirs.get(b);
            let final_path = match (o, t) {
                // Both branches moved it somewhere different. Our path wins so
                // the file stays coherent; the clashing `name` or `parent` field
                // is what turns this into a conflict further down.
                (Some(o), Some(t)) if o != t => {
                    contested.insert(b.clone());
                    o.clone()
                }
                (Some(p), _) | (None, Some(p)) => p.clone(),
                (None, None) => continue,
            };
            base_map.insert(b.clone(), final_path.clone());
            ours_map.insert(o.unwrap_or(b).clone(), final_path.clone());
            theirs_map.insert(t.unwrap_or(b).clone(), final_path);
        }

        Alignment {
            paths: HashMap::from([
                (Which::Base, base_map),
                (Which::Ours, ours_map),
                (Which::Theirs, theirs_map),
            ]),
            contested,
        }
    }

    /// Nodes whose path changed, as the base path they had against the path the
    /// merged file gives them.
    pub(crate) fn renames(&self) -> Vec<(String, String)> {
        self.paths[&Which::Base]
            .iter()
            .filter(|(before, after)| before != after)
            .map(|(before, after)| (before.clone(), after.clone()))
            .collect()
    }

    pub(crate) fn is_contested(&self, base_path: &str) -> bool {
        self.contested.contains(base_path)
    }

    /// Rewrites one node path from a side's naming into the merged naming.
    ///
    /// A path with no entry of its own is still rewritten through its longest
    /// mapped ancestor, so nodes added underneath a renamed parent follow it.
    pub(crate) fn path(&self, which: Which, path: &str) -> String {
        let table = &self.paths[&which];
        if table.is_empty() || path == "." {
            return path.to_string();
        }
        if let Some(mapped) = table.get(path) {
            return mapped.clone();
        }
        let mut cut = path.len();
        while let Some(slash) = path[..cut].rfind('/') {
            if let Some(mapped) = table.get(&path[..slash]) {
                return format!("{mapped}{}", &path[slash..]);
            }
            cut = slash;
        }
        path.to_string()
    }

    /// Rewrites an identity from a side's naming into the merged naming.
    pub(crate) fn id(&self, which: Which, id: &EntityId) -> EntityId {
        if self.paths[&which].is_empty() {
            return id.clone();
        }
        match id {
            EntityId::Node(p) => EntityId::Node(self.path(which, p)),
            EntityId::Connection(from, to, signal, method) => EntityId::Connection(
                self.path(which, from),
                self.path(which, to),
                signal.clone(),
                method.clone(),
            ),
            EntityId::Editable(p) => EntityId::Editable(self.path(which, p)),
            other => other.clone(),
        }
    }
}
