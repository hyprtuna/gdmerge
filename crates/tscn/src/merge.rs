//! Three-way merge over semantic entities.
//!
//! Both sides are reduced to changes against the base *per entity*: a node, an
//! external resource, a sub-resource, a connection. Disjoint changes are applied
//! together; only a genuine collision (the same property changed two ways, a
//! delete against a modify) becomes a conflict, and the conflict markers wrap
//! just the entity involved rather than the whole file.
//!
//! Before any of that, the three inputs are put into one shared naming, so a
//! node one branch renamed is recognised as the node the other branch edited.
//! See [`crate::align`].

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::align::{Alignment, Which};
use crate::diff::diff_scenes;
use crate::doc::{quote, Document, Rewrite, Section, SectionKind};
use crate::nodepath::{findings, Resolution, Rewriter};
use crate::scene::{node_path, EntityId, Scene};
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

/// One header field or property of a conflicting entity, as each side has it.
///
/// Values are the original source text, so they read the way they do in the
/// file. `None` means the side does not have that item at all, which is how a
/// delete against a modify shows up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    pub key: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    /// True when the two branches disagree here, which is what has to be
    /// resolved. Rows where they agree are kept for context.
    pub differs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Human-readable identity of the entity, e.g. `node Player/Sprite2D`.
    pub entity: String,
    pub detail: String,
    /// The header field or property that could not be reconciled, when the
    /// conflict came down to one of them.
    pub key: Option<String>,
    /// Every item of the entity, side by side, for presenting the conflict.
    pub rows: Vec<ConflictRow>,
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

/// Everything needed to replay a section from one side into the merged file:
/// its resource ids and its node paths both have to be rewritten into the
/// merged document's naming.
struct Replay<'a> {
    ids: Remap<'a>,
    /// Rewrites a node path written in a header field, which is a bare path.
    field_paths: &'a dyn Fn(&str) -> String,
    /// Follows node paths inside values through renames, tree aware.
    rewriter: &'a Rewriter,
    /// Where each of this side's node paths ended up.
    moved: &'a dyn Fn(&str) -> String,
    /// The common ancestor, used when a side cannot resolve a path in its own
    /// tree. A branch that renamed a node and left one of its own references
    /// behind no longer has anywhere to resolve it from, but the ancestor still
    /// does, and that is also what keeps the answer the same when the two sides
    /// are swapped.
    ancestor: &'a Rewriter,
    ancestor_moved: &'a dyn Fn(&str) -> String,
    /// Translates one of this side's node paths into the ancestor's naming.
    into_ancestor: &'a dyn Fn(&str) -> String,
}

/// Runs `render` with the rewrite that applies while replaying `section`.
///
/// The path closure has to be built here rather than handed out, because it
/// borrows both the section and the side it came from.
fn with_rewrite<R>(
    replay: &Replay<'_>,
    section: &Section,
    render: impl FnOnce(Rewrite<'_>) -> R,
) -> R {
    let paths = |path: &str| {
        let bases = replay.rewriter.bases(section);
        replay.rewriter.rewrite(&bases, path, replay.moved).or_else(|| {
            let ancestor_bases: Vec<String> =
                bases.iter().map(|b| (replay.into_ancestor)(b)).collect();
            replay.ancestor.rewrite(&ancestor_bases, path, replay.ancestor_moved)
        })
    };
    render(Rewrite { ids: replay.ids, paths: &paths })
}

/// One of the three inputs, addressed by merged identity rather than its own.
struct View<'a> {
    scene: Scene<'a>,
    which: Which,
    align: &'a Alignment,
    /// Merged identity to this document's own identity.
    local: HashMap<EntityId, EntityId>,
    /// This document's entities in file order, under merged identities.
    ids: Vec<EntityId>,
}

impl<'a> View<'a> {
    fn new(scene: Scene<'a>, which: Which, align: &'a Alignment) -> View<'a> {
        let mut local = HashMap::new();
        let mut ids = Vec::new();
        for id in scene.ids() {
            let merged = align.id(which, id);
            local.entry(merged.clone()).or_insert_with(|| id.clone());
            ids.push(merged);
        }
        View { scene, which, align, local, ids }
    }

    fn ids(&self) -> &[EntityId] {
        &self.ids
    }

    fn section(&self, id: &EntityId) -> Option<&'a Section> {
        self.scene.section(self.local.get(id)?)
    }

    /// Canonical text of the entity, used to decide whether a side changed it.
    fn canonical(&self, id: &EntityId) -> Option<String> {
        let local = self.local.get(id)?;
        self.scene.index_of(local).map(|i| self.scene.canonical(i))
    }

    fn describe(&self, id: &EntityId) -> String {
        match self.local.get(id) {
            Some(local) => self.scene.describe(local),
            None => id.describe(),
        }
    }
}

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

    let align = Alignment::new(&sb, &so, &st);
    let vb = View::new(sb, Which::Base, &align);
    let vo = View::new(so, Which::Ours, &align);
    let vt = View::new(st, Which::Theirs, &align);

    let mut plan: HashMap<EntityId, Res> = HashMap::new();
    let mut conflicts = Vec::new();

    let mut all: Vec<EntityId> = Vec::new();
    for id in vo.ids().iter().chain(vt.ids()).chain(vb.ids()) {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }

    for id in &all {
        let cb = vb.canonical(id);
        let co = vo.canonical(id);
        let ct = vt.canonical(id);

        let res = match (&cb, &co, &ct) {
            (_, None, None) => continue, // Removed by both, or never present.
            (_, Some(_), Some(_)) if co == ct => Res::Take(Side::Ours),
            (Some(_), Some(_), Some(_)) if cb == co => Res::Take(Side::Theirs),
            (Some(_), Some(_), Some(_)) if cb == ct => Res::Take(Side::Ours),
            (_, Some(_), Some(_)) => {
                // Both sides changed the same entity: try to merge item by item.
                let b = vb.section(id);
                let o = vo.section(id).expect("canonical implies a section");
                let t = vt.section(id).expect("canonical implies a section");
                match merge_items(&vb, b, &vo, o, &vt, t) {
                    Ok((fields, props)) => Res::Merge { fields, props },
                    Err(key) => {
                        conflicts.push(Conflict {
                            entity: vo.describe(id),
                            detail: clash_detail(&align, &vb, id, &key),
                            key: Some(key),
                            rows: conflict_rows(&vb, &vo, &vt, id),
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
                    entity: vo.describe(id),
                    detail: "deleted by theirs, modified by ours".to_string(),
                    key: None,
                    rows: conflict_rows(&vb, &vo, &vt, id),
                });
                Res::Conflict
            }
            (Some(_), None, Some(_)) => {
                if cb == ct {
                    continue; // We deleted it and they left it alone.
                }
                conflicts.push(Conflict {
                    entity: vt.describe(id),
                    detail: "deleted by ours, modified by theirs".to_string(),
                    key: None,
                    rows: conflict_rows(&vb, &vo, &vt, id),
                });
                Res::Conflict
            }
        };
        plan.insert(id.clone(), res);
    }

    let mut order = merged_order(&vo, &vt, &plan);
    let mut final_ids = assign_ids(&order, &vo, &vt);
    let mut text = emit(ours, &order, &plan, &vb, &vo, &vt, &final_ids, &align, opts);

    // A merge that leaves the file pointing at a node that is no longer there
    // loads without complaint and does nothing, which is worse than a conflict.
    // So it becomes one, and the file is emitted again with that entity marked.
    if conflicts.is_empty() {
        let stale = stale_references(&text, &align, &vb, &vo, &vt, &mut plan);
        if !stale.is_empty() {
            conflicts.extend(stale);
            order = merged_order(&vo, &vt, &plan);
            final_ids = assign_ids(&order, &vo, &vt);
            text = emit(ours, &order, &plan, &vb, &vo, &vt, &final_ids, &align, opts);
        }
    }

    conflicts.sort_by(|a, b| a.entity.cmp(&b.entity));
    MergeOutcome { text, conflicts }
}

/// Finds NodePaths in the merged file that name nothing, and turns each into a
/// conflict on whatever is responsible.
///
/// A path left behind by a rename is blamed on the rename, which is the change
/// that broke it and the one with two sides to choose between. Anything else is
/// reported without markers: there is no choice to offer, only a file that
/// needs a path fixed by hand.
fn stale_references(
    text: &str,
    align: &Alignment,
    vb: &View<'_>,
    vo: &View<'_>,
    vt: &View<'_>,
    plan: &mut HashMap<EntityId, Res>,
) -> Vec<Conflict> {
    let Ok(doc) = Document::parse(text) else { return Vec::new() };
    // A reference that was already broken before anyone touched the file is not
    // this merge's doing, and conflicting on it would block work nobody caused.
    // check still reports it.
    let inherited: HashSet<String> = findings(vb.scene.doc)
        .into_iter()
        .filter(|(_, outcome)| matches!(outcome, Resolution::Missing(_)))
        .map(|(reference, _)| format!("{}\u{0}{}", reference.site.describe(), reference.path))
        .collect();
    let renames: HashMap<String, String> = align.renames().into_iter().collect();
    let mut conflicts = Vec::new();
    let mut blamed: HashSet<EntityId> = HashSet::new();

    for (reference, outcome) in findings(&doc) {
        if inherited.contains(&format!("{}\u{0}{}", reference.site.describe(), reference.path)) {
            continue;
        }
        let target = match outcome {
            Resolution::Missing(target) => target,
            // A unique name with nowhere to come from is just as broken, and a
            // rename is the usual reason it stopped resolving.
            Resolution::UnknownUniqueName { name, supplied_elsewhere: false } => name,
            _ => continue,
        };
        let detail = format!(
            "NodePath(\"{}\") in {} still points at \"{}\"",
            reference.path,
            reference.site.describe(),
            target
        );
        // When the entity reported is the site itself, the detail should not
        // repeat it back.
        let alone = format!(
            "NodePath(\"{}\") points at \"{}\", which the merge removes",
            reference.path, target
        );
        // Did a branch rename this very node out from under the reference?
        match renames.get(&target) {
            Some(now) => {
                let id = EntityId::Node(now.clone());
                if !blamed.insert(id.clone()) {
                    continue;
                }
                // Only a node both branches still have can be shown as two
                // sides; otherwise fall through to the plain report.
                if vo.section(&id).is_some() && vt.section(&id).is_some() {
                    plan.insert(id.clone(), Res::Conflict);
                    conflicts.push(Conflict {
                        entity: vo.describe(&id),
                        detail: format!("renamed to \"{now}\", but {detail}"),
                        key: Some("name".to_string()),
                        rows: Vec::new(),
                    });
                    continue;
                }
                conflicts.push(Conflict {
                    entity: format!("node {now}"),
                    detail,
                    key: None,
                    rows: Vec::new(),
                });
            }
            None => conflicts.push(Conflict {
                entity: reference.site.describe(),
                detail: alone,
                key: None,
                rows: Vec::new(),
            }),
        }
    }
    conflicts
}

/// Wording for a collision, naming the rename case explicitly because the bare
/// "name changed differently" reading hides what actually happened.
fn clash_detail(align: &Alignment, vb: &View<'_>, id: &EntityId, key: &str) -> String {
    if matches!(key, "name" | "parent") {
        if let Some(section) = vb.section(id) {
            if align.is_contested(&node_path(section)) {
                return "renamed differently on both sides".to_string();
            }
        }
    }
    format!("{key} changed differently on both sides")
}

/// Lays a conflicting entity out item by item, as each of the three sides has
/// it, so the disagreement can be shown rather than only marked.
fn conflict_rows(vb: &View<'_>, vo: &View<'_>, vt: &View<'_>, id: &EntityId) -> Vec<ConflictRow> {
    let (b, o, t) = (vb.section(id), vo.section(id), vt.section(id));
    let mut rows = Vec::new();

    let mut names: Vec<String> = Vec::new();
    for s in [o, t, b].into_iter().flatten() {
        for f in &s.fields {
            if !s.kind.is_derived_field(&f.name) && !names.contains(&f.name) {
                names.push(f.name.clone());
            }
        }
    }
    for name in names {
        let ours = o.and_then(|s| canonical_field(vo, s, &name));
        let theirs = t.and_then(|s| canonical_field(vt, s, &name));
        rows.push(ConflictRow {
            differs: ours != theirs,
            base: b.and_then(|s| s.field(&name)).map(|f| f.value_raw.clone()),
            ours: o.and_then(|s| s.field(&name)).map(|f| f.value_raw.clone()),
            theirs: t.and_then(|s| s.field(&name)).map(|f| f.value_raw.clone()),
            key: name,
        });
    }

    let mut keys: Vec<String> = Vec::new();
    for s in [o, t, b].into_iter().flatten() {
        for p in &s.props {
            if !keys.contains(&p.key) {
                keys.push(p.key.clone());
            }
        }
    }
    for key in keys {
        let ours = o.and_then(|s| vo.scene.canonical_prop(s, &key));
        let theirs = t.and_then(|s| vt.scene.canonical_prop(s, &key));
        rows.push(ConflictRow {
            differs: ours != theirs,
            base: b.and_then(|s| s.prop(&key)).map(|p| p.value_raw.clone()),
            ours: o.and_then(|s| s.prop(&key)).map(|p| p.value_raw.clone()),
            theirs: t.and_then(|s| s.prop(&key)).map(|p| p.value_raw.clone()),
            key,
        });
    }
    rows
}

/// Item-level three-way merge of one section. The error names the first header
/// field or property that could not be reconciled.
#[allow(clippy::type_complexity)]
fn merge_items(
    vb: &View<'_>,
    base: Option<&Section>,
    vo: &View<'_>,
    ours: &Section,
    vt: &View<'_>,
    theirs: &Section,
) -> Result<(Vec<(String, Side)>, Vec<(String, Side)>), String> {
    let mut fields = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for f in ours.fields.iter().chain(theirs.fields.iter()) {
        if !names.contains(&f.name.as_str()) {
            names.push(&f.name);
        }
    }
    for name in names {
        let derived = ours.kind.is_derived_field(name);
        let b = base.and_then(|s| canonical_field(vb, s, name));
        let o = canonical_field(vo, ours, name);
        let t = canonical_field(vt, theirs, name);
        match pick(derived, &b, &o, &t) {
            Pick::Drop => {}
            Pick::Side(side) => fields.push((name.to_string(), side)),
            Pick::Clash => return Err(name.to_string()),
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
        let b = base.and_then(|s| vb.scene.canonical_prop(s, key));
        let o = vo.scene.canonical_prop(ours, key);
        let t = vt.scene.canonical_prop(theirs, key);
        match pick(false, &b, &o, &t) {
            Pick::Drop => {}
            Pick::Side(side) => props.push((key.to_string(), side)),
            Pick::Clash => return Err(key.to_string()),
        }
    }
    Ok((fields, props))
}

/// A header field's canonical form, with node paths read in the merged naming.
///
/// Without this, a rename on one branch makes `parent="Holder"` and
/// `parent="Box"` look like a disagreement when they name the same node.
fn canonical_field(view: &View<'_>, section: &Section, name: &str) -> Option<String> {
    let raw = view.scene.canonical_field(section, name)?;
    if !path_field(section.kind, name) {
        return Some(raw);
    }
    let value = section.field_str(name)?;
    Some(format!("{:?}", view_path(view, value)))
}

fn view_path(view: &View<'_>, path: &str) -> String {
    view.align.path(view.which, path)
}

/// Header fields whose value is a node path, and therefore has to be rewritten
/// when a node it names was renamed on the other branch.
fn path_field(kind: SectionKind, name: &str) -> bool {
    matches!(
        (kind, name),
        (SectionKind::Node, "parent")
            | (SectionKind::Connection, "from")
            | (SectionKind::Connection, "to")
            | (SectionKind::Editable, "path")
    )
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
fn merged_order(vo: &View<'_>, vt: &View<'_>, plan: &HashMap<EntityId, Res>) -> Vec<EntityId> {
    let mut order: Vec<EntityId> =
        vo.ids().iter().filter(|id| plan.contains_key(*id)).cloned().collect();
    let present: HashSet<EntityId> = order.iter().cloned().collect();

    let theirs_ids = vt.ids();
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
fn assign_ids(order: &[EntityId], vo: &View<'_>, vt: &View<'_>) -> HashMap<EntityId, String> {
    let resources: Vec<&EntityId> =
        order.iter().filter(|id| matches!(id, EntityId::Ext(_) | EntityId::Sub(_))).collect();

    let mut used: HashSet<String> = HashSet::new();
    let mut out: HashMap<EntityId, String> = HashMap::new();

    for ours_first in [true, false] {
        for (position, id) in resources.iter().enumerate() {
            let from_ours = vo.section(id).is_some();
            if from_ours != ours_first {
                continue;
            }
            let Some(section) = vo.section(id).or_else(|| vt.section(id)) else { continue };
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
    vb: &View<'_>,
    vo: &View<'_>,
    vt: &View<'_>,
    final_ids: &HashMap<EntityId, String>,
    align: &Alignment,
    opts: &MergeOptions,
) -> String {
    let ext_count = order.iter().filter(|id| matches!(id, EntityId::Ext(_))).count();
    let sub_count = order.iter().filter(|id| matches!(id, EntityId::Sub(_))).count();
    let load_steps = ext_count + sub_count + 1;

    let map_ours = remapper(&vo.scene, final_ids);
    let map_theirs = remapper(&vt.scene, final_ids);
    let paths_ours = |p: &str| align.path(Which::Ours, p);
    let paths_theirs = |p: &str| align.path(Which::Theirs, p);
    let rw_ours = Rewriter::new(vo.scene.doc);
    let rw_theirs = Rewriter::new(vt.scene.doc);
    let rw_base = Rewriter::new(vb.scene.doc);
    let paths_base = |p: &str| align.path(Which::Base, p);
    let ours_into_base = |p: &str| {
        let merged = align.path(Which::Ours, p);
        align.base_path_of(&merged).unwrap_or(merged)
    };
    let theirs_into_base = |p: &str| {
        let merged = align.path(Which::Theirs, p);
        align.base_path_of(&merged).unwrap_or(merged)
    };
    let r_ours = Replay {
        ids: &map_ours,
        field_paths: &paths_ours,
        rewriter: &rw_ours,
        moved: &paths_ours,
        ancestor: &rw_base,
        ancestor_moved: &paths_base,
        into_ancestor: &ours_into_base,
    };
    let r_theirs = Replay {
        ids: &map_theirs,
        field_paths: &paths_theirs,
        rewriter: &rw_theirs,
        moved: &paths_theirs,
        ancestor: &rw_base,
        ancestor_moved: &paths_base,
        into_ancestor: &theirs_into_base,
    };
    let nl = ours.newline();

    let mut out = String::new();
    out.push_str(&ours.lead);
    for (i, id) in order.iter().enumerate() {
        match plan.get(id) {
            Some(Res::Take(side)) => {
                let (view, replay) = match side {
                    Side::Ours => (vo, &r_ours),
                    Side::Theirs => (vt, &r_theirs),
                };
                let section = view.section(id).expect("plan refers to a real section");
                out.push_str(&render_section(section, id, final_ids, replay, load_steps, nl));
            }
            Some(Res::Merge { fields, props }) => {
                out.push_str(&render_merged(
                    id, fields, props, vo, vt, &r_ours, &r_theirs, final_ids, load_steps, nl,
                ));
            }
            Some(Res::Conflict) => {
                let mine = vo
                    .section(id)
                    .map(|s| render_section(s, id, final_ids, &r_ours, load_steps, nl));
                let yours = vt
                    .section(id)
                    .map(|s| render_section(s, id, final_ids, &r_theirs, load_steps, nl));
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
    replay: &Replay<'_>,
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
        out.push_str(&field_text(
            section,
            id,
            &f.name,
            &f.value_raw,
            final_ids,
            replay,
            load_steps,
        ));
    }
    out.push_str(&section.close_sep);
    out.push(']');
    for p in &section.props {
        push_lead(&mut out, &p.lead, nl);
        out.push_str(&p.key_raw);
        out.push_str(&p.sep_eq);
        out.push('=');
        out.push_str(&p.sep_val);
        out.push_str(&with_rewrite(replay, section, |rw| p.rendered(rw)));
    }
    out
}

/// Header field text, with derived fields substituted and node paths rewritten
/// into the merged naming.
#[allow(clippy::too_many_arguments)]
fn field_text(
    section: &Section,
    id: &EntityId,
    name: &str,
    raw: &str,
    final_ids: &HashMap<EntityId, String>,
    replay: &Replay<'_>,
    load_steps: usize,
) -> String {
    match (section.kind, name) {
        (SectionKind::GdScene | SectionKind::GdResource, "load_steps") => load_steps.to_string(),
        (SectionKind::ExtResource | SectionKind::SubResource, "id") => match final_ids.get(id) {
            Some(new_id) => format!("\"{new_id}\""),
            None => raw.to_string(),
        },
        _ if path_field(section.kind, name) => {
            let current = section.field_str(name).unwrap_or_default();
            let mapped = (replay.field_paths)(current);
            if mapped == current {
                raw.to_string()
            } else {
                quote(&mapped)
            }
        }
        _ => section
            .field(name)
            .map(|f| with_rewrite(replay, section, |rw| f.rendered(rw)))
            .unwrap_or_else(|| raw.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_merged(
    id: &EntityId,
    fields: &[(String, Side)],
    props: &[(String, Side)],
    vo: &View<'_>,
    vt: &View<'_>,
    r_ours: &Replay<'_>,
    r_theirs: &Replay<'_>,
    final_ids: &HashMap<EntityId, String>,
    load_steps: usize,
    nl: &str,
) -> String {
    let ours = vo.section(id);
    let theirs = vt.section(id);
    let skeleton = ours.or(theirs).expect("a merged entity exists on at least one side");

    let mut out = String::new();
    out.push('[');
    out.push_str(&skeleton.tag);
    for (name, side) in fields {
        let (section, replay) = match side {
            Side::Ours => (ours, r_ours),
            Side::Theirs => (theirs, r_theirs),
        };
        let Some(section) = section else { continue };
        let Some(f) = section.field(name) else { continue };
        out.push(' ');
        out.push_str(name);
        out.push('=');
        out.push_str(&field_text(section, id, name, &f.value_raw, final_ids, replay, load_steps));
    }
    out.push(']');
    for (key, side) in props {
        let (section, replay) = match side {
            Side::Ours => (ours, r_ours),
            Side::Theirs => (theirs, r_theirs),
        };
        let Some(section) = section else { continue };
        let Some(p) = section.prop(key) else { continue };
        push_lead(&mut out, &p.lead, nl);
        out.push_str(&p.key_raw);
        out.push_str(&p.sep_eq);
        out.push('=');
        out.push_str(&p.sep_val);
        out.push_str(&with_rewrite(replay, section, |rw| p.rendered(rw)));
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
