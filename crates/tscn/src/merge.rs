//! Three-way merge over semantic entities.
//!
//! Both sides are reduced to changes against the base *per entity*: a node, an
//! external resource, a sub-resource, a connection. Disjoint changes are applied
//! together; only a genuine collision (the same property changed two ways, a
//! delete against a modify) becomes a conflict, and the conflict markers wrap
//! just the entity involved rather than the whole file.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::diff::diff_scenes;
use crate::doc::{Document, Section, SectionKind};
use crate::scene::{EntityId, Scene};
use crate::value::RefKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Ours,
    Theirs,
}

#[derive(Debug, Clone)]
pub struct MergeOptions {
    pub ours_label: String,
    pub theirs_label: String,
    /// Length of the `<<<<<<<` run; git passes this as `%L`.
    pub marker_size: usize,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            ours_label: "ours".to_string(),
            theirs_label: "theirs".to_string(),
            marker_size: 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Human-readable identity of the entity, e.g. `node Player/Sprite2D`.
    pub entity: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub text: String,
    pub conflicts: Vec<Conflict>,
}

impl MergeOutcome {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// A resource-id rewriting function, applied while rendering a section that
/// came from one particular side.
type Remap<'a> = &'a dyn Fn(RefKind, &str) -> Option<String>;

/// One resolved entity in the merged document.
enum Res {
    /// Take the whole section from one side.
    Take(Side),
    /// Rebuild the section from per-item picks: (field name, side) and (property key, side).
    Merge {
        fields: Vec<(String, Side)>,
        props: Vec<(String, Side)>,
    },
    Conflict,
}

/// Merges `ours` and `theirs` against their common ancestor `base`.
pub fn merge(
    base: &Document,
    ours: &Document,
    theirs: &Document,
    opts: &MergeOptions,
) -> MergeOutcome {
    let sb = Scene::new(base);
    let so = Scene::new(ours);
    let st = Scene::new(theirs);

    // Whole-side fast paths. Keeping one side's bytes verbatim is both the
    // right answer and the strongest guarantee we can offer: a merge where the
    // other branch changed nothing semantic cannot reformat the file.
    if diff_scenes(&sb, &st).is_empty() {
        return MergeOutcome { text: ours.to_source(), conflicts: Vec::new() };
    }
    if diff_scenes(&sb, &so).is_empty() {
        return MergeOutcome { text: theirs.to_source(), conflicts: Vec::new() };
    }
    if diff_scenes(&so, &st).is_empty() {
        return MergeOutcome { text: ours.to_source(), conflicts: Vec::new() };
    }

    let mut plan: HashMap<EntityId, Res> = HashMap::new();
    let mut conflicts = Vec::new();

    let mut all: Vec<EntityId> = Vec::new();
    for id in so.ids().iter().chain(st.ids()).chain(sb.ids()) {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }

    for id in &all {
        let cb = sb.index_of(id).map(|i| sb.canonical(i));
        let co = so.index_of(id).map(|i| so.canonical(i));
        let ct = st.index_of(id).map(|i| st.canonical(i));

        let res = match (&cb, &co, &ct) {
            (_, None, None) => continue, // Removed by both, or never present.
            (_, Some(_), Some(_)) if co == ct => Res::Take(Side::Ours),
            (Some(_), Some(_), Some(_)) if cb == co => Res::Take(Side::Theirs),
            (Some(_), Some(_), Some(_)) if cb == ct => Res::Take(Side::Ours),
            (_, Some(_), Some(_)) => {
                // Both sides changed the same entity: try to merge item by item.
                let b = sb.section(id);
                let o = so.section(id).expect("canonical implies a section");
                let t = st.section(id).expect("canonical implies a section");
                match merge_items(&sb, b, &so, o, &st, t) {
                    Some((fields, props)) => Res::Merge { fields, props },
                    None => {
                        conflicts.push(Conflict {
                            entity: so.describe(id),
                            detail: "changed differently on both sides".to_string(),
                        });
                        Res::Conflict
                    }
                }
            }
            (None, Some(_), None) => Res::Take(Side::Ours), // We added it.
            (None, None, Some(_)) => Res::Take(Side::Theirs), // They added it.
            (Some(_), Some(_), None) => {
                if cb == co {
                    continue; // They deleted it and we left it alone.
                }
                conflicts.push(Conflict {
                    entity: so.describe(id),
                    detail: "deleted by theirs, modified by ours".to_string(),
                });
                Res::Conflict
            }
            (Some(_), None, Some(_)) => {
                if cb == ct {
                    continue; // We deleted it and they left it alone.
                }
                conflicts.push(Conflict {
                    entity: st.describe(id),
                    detail: "deleted by ours, modified by theirs".to_string(),
                });
                Res::Conflict
            }
        };
        plan.insert(id.clone(), res);
    }

    let order = merged_order(&so, &st, &plan);
    let final_ids = assign_ids(&order, &so, &st);
    let text = emit(ours, &order, &plan, &so, &st, &final_ids, opts);
    conflicts.sort_by(|a, b| a.entity.cmp(&b.entity));
    MergeOutcome { text, conflicts }
}

/// Item-level three-way merge of one section. `None` means a real collision.
#[allow(clippy::type_complexity)]
fn merge_items(
    sb: &Scene<'_>,
    base: Option<&Section>,
    so: &Scene<'_>,
    ours: &Section,
    st: &Scene<'_>,
    theirs: &Section,
) -> Option<(Vec<(String, Side)>, Vec<(String, Side)>)> {
    let mut fields = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for f in ours.fields.iter().chain(theirs.fields.iter()) {
        if !names.contains(&f.name.as_str()) {
            names.push(&f.name);
        }
    }
    for name in names {
        let derived = ours.kind.is_derived_field(name);
        let b = base.and_then(|s| sb.canonical_field(s, name));
        let o = so.canonical_field(ours, name);
        let t = st.canonical_field(theirs, name);
        match pick(derived, &b, &o, &t) {
            Pick::Drop => {}
            Pick::Side(side) => fields.push((name.to_string(), side)),
            Pick::Clash => return None,
        }
    }

    let mut props = Vec::new();
    let mut keys: Vec<&str> = Vec::new();
    for p in ours.props.iter().chain(theirs.props.iter()) {
        if !keys.contains(&p.key.as_str()) {
            keys.push(&p.key);
        }
    }
    for key in keys {
        let b = base.and_then(|s| sb.canonical_prop(s, key));
        let o = so.canonical_prop(ours, key);
        let t = st.canonical_prop(theirs, key);
        match pick(false, &b, &o, &t) {
            Pick::Drop => {}
            Pick::Side(side) => props.push((key.to_string(), side)),
            Pick::Clash => return None,
        }
    }
    Some((fields, props))
}

enum Pick {
    Side(Side),
    Drop,
    Clash,
}

fn pick(derived: bool, b: &Option<String>, o: &Option<String>, t: &Option<String>) -> Pick {
    if derived {
        // Recomputed at emit time; presence follows whichever side still has it.
        return match (o, t) {
            (Some(_), _) => Pick::Side(Side::Ours),
            (None, Some(_)) => Pick::Side(Side::Theirs),
            (None, None) => Pick::Drop,
        };
    }
    if o == t {
        return match o {
            Some(_) => Pick::Side(Side::Ours),
            None => Pick::Drop,
        };
    }
    if b == o {
        return match t {
            Some(_) => Pick::Side(Side::Theirs),
            None => Pick::Drop,
        };
    }
    if b == t {
        return match o {
            Some(_) => Pick::Side(Side::Ours),
            None => Pick::Drop,
        };
    }
    Pick::Clash
}

/// Final section order: ours' order, with theirs-only entities slotted in after
/// the neighbour they follow in theirs, then grouped by section class.
fn merged_order(so: &Scene<'_>, st: &Scene<'_>, plan: &HashMap<EntityId, Res>) -> Vec<EntityId> {
    let mut order: Vec<EntityId> =
        so.ids().iter().filter(|id| plan.contains_key(*id)).cloned().collect();
    let present: HashSet<EntityId> = order.iter().cloned().collect();

    let theirs_ids = st.ids();
    for (i, id) in theirs_ids.iter().enumerate() {
        if present.contains(id) || !plan.contains_key(id) || order.contains(id) {
            continue;
        }
        let at = theirs_ids[..i]
            .iter()
            .rev()
            .find_map(|prev| order.iter().position(|x| x == prev).map(|p| p + 1))
            .unwrap_or_else(|| {
                order.iter().position(|x| x.rank() > id.rank()).unwrap_or(order.len())
            });
        order.insert(at, id.clone());
    }
    order.sort_by_key(|id| id.rank()); // Stable: keeps relative order within a class.
    order
}

/// Assigns the resource ids of the merged file.
///
/// Our side's ids are claimed first so that merging never renumbers resources
/// that were already in our file; only resources arriving from their side are
/// renamed, and only when their id would collide.
fn assign_ids(order: &[EntityId], so: &Scene<'_>, st: &Scene<'_>) -> HashMap<EntityId, String> {
    let resources: Vec<&EntityId> =
        order.iter().filter(|id| matches!(id, EntityId::Ext(_) | EntityId::Sub(_))).collect();

    let mut used: HashSet<String> = HashSet::new();
    let mut out: HashMap<EntityId, String> = HashMap::new();

    for ours_first in [true, false] {
        for (position, id) in resources.iter().enumerate() {
            let from_ours = so.section(id).is_some();
            if from_ours != ours_first {
                continue;
            }
            let Some(section) = so.section(id).or_else(|| st.section(id)) else { continue };
            let preferred = section.field_str("id").unwrap_or_default();
            if !preferred.is_empty() && used.insert(preferred.to_string()) {
                out.insert((*id).clone(), preferred.to_string());
                continue;
            }
            let stem = match id {
                EntityId::Sub(_) => section.field_str("type").unwrap_or("SubResource").to_string(),
                _ => (position + 1).to_string(),
            };
            let mut n = 0;
            let minted = loop {
                let candidate = format!("{stem}_gdm{n}");
                if used.insert(candidate.clone()) {
                    break candidate;
                }
                n += 1;
            };
            out.insert((*id).clone(), minted);
        }
    }
    out
}

fn remapper<'a>(
    scene: &'a Scene<'a>,
    final_ids: &'a HashMap<EntityId, String>,
) -> impl Fn(RefKind, &str) -> Option<String> + 'a {
    move |kind, local| {
        let key = scene.key_for(kind, local)?;
        let entity = match kind {
            RefKind::Ext => EntityId::Ext(key.to_string()),
            RefKind::Sub => EntityId::Sub(key.to_string()),
        };
        final_ids.get(&entity).cloned()
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    ours: &Document,
    order: &[EntityId],
    plan: &HashMap<EntityId, Res>,
    so: &Scene<'_>,
    st: &Scene<'_>,
    final_ids: &HashMap<EntityId, String>,
    opts: &MergeOptions,
) -> String {
    let ext_count = order.iter().filter(|id| matches!(id, EntityId::Ext(_))).count();
    let sub_count = order.iter().filter(|id| matches!(id, EntityId::Sub(_))).count();
    let load_steps = ext_count + sub_count + 1;

    let map_ours = remapper(so, final_ids);
    let map_theirs = remapper(st, final_ids);
    let nl = ours.newline();

    let mut out = String::new();
    out.push_str(&ours.lead);
    for (i, id) in order.iter().enumerate() {
        match plan.get(id) {
            Some(Res::Take(side)) => {
                let (scene, map): (&Scene<'_>, Remap<'_>) = match side {
                    Side::Ours => (so, &map_ours),
                    Side::Theirs => (st, &map_theirs),
                };
                let section = scene.section(id).expect("plan refers to a real section");
                out.push_str(&render_section(section, id, final_ids, map, load_steps, nl));
            }
            Some(Res::Merge { fields, props }) => {
                out.push_str(&render_merged(
                    id,
                    fields,
                    props,
                    so,
                    st,
                    &map_ours,
                    &map_theirs,
                    final_ids,
                    load_steps,
                    nl,
                ));
            }
            Some(Res::Conflict) => {
                let mine = so
                    .section(id)
                    .map(|s| render_section(s, id, final_ids, &map_ours, load_steps, nl));
                let yours = st
                    .section(id)
                    .map(|s| render_section(s, id, final_ids, &map_theirs, load_steps, nl));
                let m = "<".repeat(opts.marker_size);
                let e = "=".repeat(opts.marker_size);
                let g = ">".repeat(opts.marker_size);
                let _ = write!(out, "{m} {}{nl}", opts.ours_label);
                if let Some(body) = mine {
                    out.push_str(&body);
                    out.push_str(nl);
                }
                let _ = write!(out, "{e}{nl}");
                if let Some(body) = yours {
                    out.push_str(&body);
                    out.push_str(nl);
                }
                let _ = write!(out, "{g} {}", opts.theirs_label);
            }
            None => continue,
        }
        for _ in 0..separator_lines(id, order.get(i + 1)) {
            out.push_str(nl);
        }
    }
    out
}

/// How many line breaks go between two sections, matching Godot's own writer:
/// consecutive resource, connection and editable entries are packed together,
/// everything else is separated by a blank line.
fn separator_lines(current: &EntityId, next: Option<&EntityId>) -> usize {
    let Some(next) = next else { return 1 };
    match (current, next) {
        (EntityId::Ext(_), EntityId::Ext(_)) => 1,
        (EntityId::Connection(..), EntityId::Connection(..)) => 1,
        (EntityId::Editable(_), EntityId::Editable(_)) => 1,
        _ => 2,
    }
}

fn render_section(
    section: &Section,
    id: &EntityId,
    final_ids: &HashMap<EntityId, String>,
    map: Remap<'_>,
    load_steps: usize,
    nl: &str,
) -> String {
    let mut out = String::new();
    out.push('[');
    out.push_str(&section.open_sep);
    out.push_str(&section.tag);
    for f in &section.fields {
        out.push_str(&f.sep_before);
        out.push_str(&f.name);
        out.push_str(&f.sep_eq);
        out.push('=');
        out.push_str(&f.sep_val);
        out.push_str(&field_text(section, id, &f.name, &f.value_raw, final_ids, map, load_steps));
    }
    out.push_str(&section.close_sep);
    out.push(']');
    for p in &section.props {
        push_lead(&mut out, &p.lead, nl);
        out.push_str(&p.key_raw);
        out.push_str(&p.sep_eq);
        out.push('=');
        out.push_str(&p.sep_val);
        out.push_str(&p.rendered(map));
    }
    out
}

/// Header field text, with the two derived fields substituted.
#[allow(clippy::too_many_arguments)]
fn field_text(
    section: &Section,
    id: &EntityId,
    name: &str,
    raw: &str,
    final_ids: &HashMap<EntityId, String>,
    map: Remap<'_>,
    load_steps: usize,
) -> String {
    match (section.kind, name) {
        (SectionKind::GdScene | SectionKind::GdResource, "load_steps") => load_steps.to_string(),
        (SectionKind::ExtResource | SectionKind::SubResource, "id") => match final_ids.get(id) {
            Some(new_id) => format!("\"{new_id}\""),
            None => raw.to_string(),
        },
        _ => section.field(name).map(|f| f.rendered(map)).unwrap_or_else(|| raw.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_merged(
    id: &EntityId,
    fields: &[(String, Side)],
    props: &[(String, Side)],
    so: &Scene<'_>,
    st: &Scene<'_>,
    map_ours: Remap<'_>,
    map_theirs: Remap<'_>,
    final_ids: &HashMap<EntityId, String>,
    load_steps: usize,
    nl: &str,
) -> String {
    let ours = so.section(id);
    let theirs = st.section(id);
    let skeleton = ours.or(theirs).expect("a merged entity exists on at least one side");

    let mut out = String::new();
    out.push('[');
    out.push_str(&skeleton.tag);
    for (name, side) in fields {
        let (section, map) = match side {
            Side::Ours => (ours, map_ours),
            Side::Theirs => (theirs, map_theirs),
        };
        let Some(section) = section else { continue };
        let Some(f) = section.field(name) else { continue };
        out.push(' ');
        out.push_str(name);
        out.push('=');
        out.push_str(&field_text(section, id, name, &f.value_raw, final_ids, map, load_steps));
    }
    out.push(']');
    for (key, side) in props {
        let (section, map) = match side {
            Side::Ours => (ours, map_ours),
            Side::Theirs => (theirs, map_theirs),
        };
        let Some(section) = section else { continue };
        let Some(p) = section.prop(key) else { continue };
        push_lead(&mut out, &p.lead, nl);
        out.push_str(&p.key_raw);
        out.push_str(&p.sep_eq);
        out.push('=');
        out.push_str(&p.sep_val);
        out.push_str(&p.rendered(map));
    }
    out
}

/// Property leads carry the newline that starts the line. A pick taken from the
/// other side could in principle arrive without one, so guarantee it.
fn push_lead(out: &mut String, lead: &str, nl: &str) {
    if !lead.contains('\n') {
        out.push_str(nl);
    }
    out.push_str(lead);
}
