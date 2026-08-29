//! Semantic diff between two documents.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::doc::{Document, Section};
use crate::scene::{EntityId, Scene};

/// A single header field or property that differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PropertyChange {
    pub key: String,
    /// Source text of the old value; `None` when the key was added.
    pub before: Option<String>,
    /// Source text of the new value; `None` when the key was removed.
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Added {
        entity: String,
        #[serde(skip)]
        id: EntityId,
    },
    Removed {
        entity: String,
        #[serde(skip)]
        id: EntityId,
    },
    /// A node that kept its contents but changed name and/or parent.
    Moved {
        from: String,
        to: String,
        #[serde(skip)]
        id: EntityId,
    },
    Modified {
        entity: String,
        fields: Vec<PropertyChange>,
        properties: Vec<PropertyChange>,
        #[serde(skip)]
        id: EntityId,
    },
    /// Same entity, different position among its siblings.
    Reordered {
        entity: String,
        from: usize,
        to: usize,
        #[serde(skip)]
        id: EntityId,
    },
}

impl Change {
    pub fn id(&self) -> &EntityId {
        match self {
            Change::Added { id, .. }
            | Change::Removed { id, .. }
            | Change::Moved { id, .. }
            | Change::Modified { id, .. }
            | Change::Reordered { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Computes the semantic difference between two parsed documents.
pub fn diff(before: &Document, after: &Document) -> Diff {
    let a = Scene::new(before);
    let b = Scene::new(after);
    diff_scenes(&a, &b)
}

pub(crate) fn diff_scenes(a: &Scene<'_>, b: &Scene<'_>) -> Diff {
    let a_ids: BTreeSet<EntityId> = a.ids().iter().cloned().collect();
    let b_ids: BTreeSet<EntityId> = b.ids().iter().cloned().collect();

    let mut removed: Vec<EntityId> = a_ids.difference(&b_ids).cloned().collect();
    let mut added: Vec<EntityId> = b_ids.difference(&a_ids).cloned().collect();
    let mut changes = Vec::new();

    for (from, to) in detect_moves(a, b, &removed, &added) {
        removed.retain(|id| *id != from);
        added.retain(|id| *id != to);
        changes.push(Change::Moved {
            from: a.describe(&from),
            to: b.describe(&to),
            id: to.clone(),
        });
    }

    for id in &removed {
        changes.push(Change::Removed { entity: a.describe(id), id: id.clone() });
    }
    for id in &added {
        changes.push(Change::Added { entity: b.describe(id), id: id.clone() });
    }

    for id in a_ids.intersection(&b_ids) {
        let (Some(sa), Some(sb)) = (a.section(id), b.section(id)) else { continue };
        let (fields, properties) = section_delta(a, sa, b, sb);
        if !fields.is_empty() || !properties.is_empty() {
            changes.push(Change::Modified {
                entity: b.describe(id),
                fields,
                properties,
                id: id.clone(),
            });
        }
    }

    changes.extend(reorderings(a, b, &a_ids, &b_ids));
    changes.sort_by(|x, y| {
        (x.id().rank(), x.id().describe()).cmp(&(y.id().rank(), y.id().describe()))
    });
    Diff { changes }
}

/// Field/property level delta between two sections of the same identity.
fn section_delta(
    a: &Scene<'_>,
    sa: &Section,
    b: &Scene<'_>,
    sb: &Section,
) -> (Vec<PropertyChange>, Vec<PropertyChange>) {
    let mut fields = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for f in sa.fields.iter().chain(sb.fields.iter()) {
        if !names.contains(&f.name.as_str()) {
            names.push(&f.name);
        }
    }
    for name in names {
        if sa.kind.is_derived_field(name) {
            continue;
        }
        let ca = a.canonical_field(sa, name);
        let cb = b.canonical_field(sb, name);
        if ca != cb {
            fields.push(PropertyChange {
                key: name.to_string(),
                before: sa.field(name).map(|f| f.value_raw.clone()),
                after: sb.field(name).map(|f| f.value_raw.clone()),
            });
        }
    }

    let mut props = Vec::new();
    let mut keys: Vec<&str> = Vec::new();
    for p in sa.props.iter().chain(sb.props.iter()) {
        if !keys.contains(&p.key.as_str()) {
            keys.push(&p.key);
        }
    }
    for key in keys {
        let ca = a.canonical_prop(sa, key);
        let cb = b.canonical_prop(sb, key);
        if ca != cb {
            props.push(PropertyChange {
                key: key.to_string(),
                before: sa.prop(key).map(|p| p.value_raw.clone()),
                after: sb.prop(key).map(|p| p.value_raw.clone()),
            });
        }
    }
    (fields, props)
}

/// Pairs removed and added nodes whose contents are identical: a rename, a
/// reparent, or both. Deliberately conservative — contents must match exactly.
fn detect_moves(
    a: &Scene<'_>,
    b: &Scene<'_>,
    removed: &[EntityId],
    added: &[EntityId],
) -> Vec<(EntityId, EntityId)> {
    let mut by_content: HashMap<String, Vec<&EntityId>> = HashMap::new();
    for id in added {
        if !matches!(id, EntityId::Node(_)) {
            continue;
        }
        let Some(i) = b.index_of(id) else { continue };
        by_content.entry(content_key(b, i)).or_default().push(id);
    }

    let mut taken: HashSet<&EntityId> = HashSet::new();
    let mut pairs = Vec::new();
    for from in removed {
        if !matches!(from, EntityId::Node(_)) {
            continue;
        }
        let Some(i) = a.index_of(from) else { continue };
        let Some(candidates) = by_content.get(&content_key(a, i)) else { continue };
        if let Some(to) = candidates.iter().find(|c| !taken.contains(**c)) {
            taken.insert(to);
            pairs.push((from.clone(), (*to).clone()));
        }
    }
    pairs
}

/// Canonical content of a node ignoring the fields that define its identity.
fn content_key(scene: &Scene<'_>, index: usize) -> String {
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

/// Entities whose position among their own kind changed.
fn reorderings(
    a: &Scene<'_>,
    scene_b: &Scene<'_>,
    a_ids: &BTreeSet<EntityId>,
    b_ids: &BTreeSet<EntityId>,
) -> Vec<Change> {
    let pos_a = positions(a, a_ids, b_ids);
    let pos_b = positions(scene_b, b_ids, a_ids);
    let mut out = Vec::new();
    for (id, from) in &pos_a {
        // Only report a *relative* move: a shift caused purely by an insertion
        // or deletion elsewhere is not itself a reordering.
        if let Some(to) = pos_b.get(id) {
            if from != to {
                out.push(Change::Reordered {
                    entity: scene_b.describe(id),
                    from: *from,
                    to: *to,
                    id: id.clone(),
                });
            }
        }
    }
    out
}

/// Index of each shared entity among the shared entities of the same kind.
fn positions(
    scene: &Scene<'_>,
    own: &BTreeSet<EntityId>,
    other: &BTreeSet<EntityId>,
) -> HashMap<EntityId, usize> {
    let _ = own;
    let mut counters: HashMap<u8, usize> = HashMap::new();
    let mut out = HashMap::new();
    for id in scene.ids() {
        if !other.contains(id) {
            continue;
        }
        let n = counters.entry(id.rank()).or_insert(0);
        out.insert(id.clone(), *n);
        *n += 1;
    }
    out
}
