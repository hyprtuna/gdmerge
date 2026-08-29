//! Resolving `NodePath` values against the scene tree the file describes.
//!
//! A merge that renames a node and leaves a `NodePath` pointing at the old name
//! produces a scene that loads but is wired to nothing. Catching that means
//! actually walking the paths, which is more involved than it looks: a path is
//! relative to whatever holds it, most of them live in animation sub-resources
//! that are not in the tree at all, and several forms cannot be decided from one
//! file. The rule throughout is that anything uncertain is a warning and only a
//! path that definitely resolves to nothing is an error, because a false alarm
//! on a valid scene is worse than a missed one.

use std::collections::{HashMap, HashSet};

use crate::doc::{Document, Section, SectionKind};
use crate::scene::node_path;
use crate::value::RefKind;

/// Where a path ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// Names a node this file declares.
    Node(String),
    /// Leaves the file: absolute, or walks above the root. A parent scene
    /// decides what it means, so it cannot be judged here.
    Outside,
    /// Lands inside a scene this file instances, whose contents live elsewhere.
    Instanced,
    /// A unique name (`%Thing`) this file does not declare. `supplied_elsewhere`
    /// is true when the file instances a scene that could be providing it, in
    /// which case nothing can be concluded; when it is false the name is
    /// definitely not there.
    UnknownUniqueName { name: String, supplied_elsewhere: bool },
    /// Resolves to a path this file does not have. This is the real failure.
    Missing(String),
}

/// What a reference is attached to, for the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Site {
    NodeProperty { node: String, property: String },
    SubResourceProperty { id: String, property: String },
    Connection { field: &'static str, from: String, to: String },
    Editable,
}

impl Site {
    pub(crate) fn describe(&self) -> String {
        match self {
            Site::NodeProperty { node, property } => {
                format!("{property} on node \"{node}\"")
            }
            Site::SubResourceProperty { id, property } => {
                format!("{property} on sub_resource \"{id}\"")
            }
            Site::Connection { field, from, to } => {
                format!("the {field} of the connection from \"{from}\" to \"{to}\"")
            }
            Site::Editable => "an [editable] path".to_string(),
        }
    }
}

/// Whether the scene root is a sensible thing to measure a path from.
///
/// Which node a path is relative to depends on the property holding it, and
/// Godot has no shortage of them: an animation track resolves against
/// `root_node`, a `ViewportTexture` against the scene root, an exported
/// property against the node itself. Rather than encode that table and be wrong
/// about the entries it misses, a path is accepted if it resolves from any
/// plausible base. A path beginning with `..` is the exception: it is only
/// meaningful from a node, so the root is not offered and the common case of a
/// sibling reference left behind by a rename is still caught.
pub(crate) fn root_is_plausible(path: &str) -> bool {
    !path.starts_with("..")
}

/// One `NodePath` found in the file, with the nodes it could be relative to.
#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub(crate) site: Site,
    pub(crate) path: String,
    /// A path is accepted if it resolves from any of these. Sub-resources are
    /// shared, so one can genuinely have several.
    pub(crate) bases: Vec<String>,
}

/// The node tree of a single document.
pub(crate) struct Tree {
    nodes: HashSet<String>,
    /// Nodes that instance another scene, whose children live in that scene.
    instanced: Vec<String>,
    /// `unique_name_in_owner` nodes, by the name a `%` path would use.
    unique: HashMap<String, String>,
}

fn segments(path: &str) -> Vec<&str> {
    if path == "." || path.is_empty() {
        return Vec::new();
    }
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn join(parts: &[&str]) -> String {
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

impl Tree {
    pub(crate) fn new(doc: &Document) -> Tree {
        let mut nodes = HashSet::new();
        let mut instanced = Vec::new();
        let mut unique = HashMap::new();
        for s in doc.sections_of(SectionKind::Node) {
            let path = node_path(s);
            if s.field("instance").is_some() || s.field("instance_placeholder").is_some() {
                instanced.push(path.clone());
            }
            if matches!(s.prop("unique_name_in_owner").map(|p| &p.value), Some(v) if is_true(v)) {
                if let Some(name) = s.field_str("name") {
                    unique.insert(name.to_string(), path.clone());
                }
            }
            nodes.insert(path);
        }
        Tree { nodes, instanced, unique }
    }

    fn inside_instance(&self, path: &str) -> bool {
        self.instanced.iter().any(|i| i == "." || path == i || path.starts_with(&format!("{i}/")))
    }

    /// Walks `path` from `base`, the way Godot's `get_node` would.
    pub(crate) fn resolve(&self, base: &str, path: &str) -> Resolution {
        // Everything from the first colon on selects a property, not a node.
        let node_part = path.split(':').next().unwrap_or("");
        if path.is_empty() {
            return Resolution::Outside;
        }
        if node_part.is_empty() {
            // ":property" means a property of the base node itself.
            return Resolution::Node(base.to_string());
        }
        if node_part.starts_with('/') {
            return Resolution::Outside;
        }

        let mut here: Vec<&str> = segments(base);
        for part in node_part.split('/').filter(|s| !s.is_empty()) {
            if part == "." {
                continue;
            }
            if part == ".." {
                if here.pop().is_none() {
                    // Above the root of this scene; a parent scene decides.
                    return Resolution::Outside;
                }
                continue;
            }
            if let Some(name) = part.strip_prefix('%') {
                match self.unique.get(name) {
                    Some(target) => {
                        here = segments(target);
                        continue;
                    }
                    None => {
                        return Resolution::UnknownUniqueName {
                            name: name.to_string(),
                            supplied_elsewhere: !self.instanced.is_empty(),
                        }
                    }
                }
            }
            here.push(part);
        }

        // Whether the path passed through an instanced node on the way does not
        // matter: a node declared here is an override of one inside that scene,
        // and is a perfectly good target. Only a target this file does not
        // declare has to fall back to asking whether an instance supplies it.
        let target = join(&here);
        if self.nodes.contains(&target) {
            return Resolution::Node(target);
        }
        if self.inside_instance(&target) {
            return Resolution::Instanced;
        }
        Resolution::Missing(target)
    }
}

fn is_true(v: &crate::value::Value) -> bool {
    matches!(v, crate::value::Value::Ident(s) if s == "true")
}

/// Every `NodePath` in the document, with what it is relative to.
pub(crate) fn references(doc: &Document) -> Vec<Reference> {
    let sub_bases = sub_resource_bases(doc);
    let mut out = Vec::new();

    for s in &doc.sections {
        match s.kind {
            SectionKind::Node => {
                let base = node_path(s);
                let bases = node_bases(s, &base, false);
                for p in &s.props {
                    collect(&p.value, &mut |path| {
                        out.push(Reference {
                            site: Site::NodeProperty {
                                node: base.clone(),
                                property: p.key.clone(),
                            },
                            path: path.to_string(),
                            bases: bases.clone(),
                        })
                    });
                }
            }
            SectionKind::SubResource => {
                let Some(id) = s.field_str("id") else { continue };
                let Some(bases) = sub_bases.get(id) else { continue };
                if bases.is_empty() {
                    continue;
                }
                // Sorted: these come out of a set, and an unstable order would
                // make which base is reported, and therefore the merge itself,
                // differ between runs.
                let mut bases: Vec<String> = bases.iter().cloned().collect();
                bases.sort();
                for p in &s.props {
                    collect(&p.value, &mut |path| {
                        out.push(Reference {
                            site: Site::SubResourceProperty {
                                id: id.to_string(),
                                property: p.key.clone(),
                            },
                            path: path.to_string(),
                            bases: bases.clone(),
                        })
                    });
                }
            }
            SectionKind::Connection => {
                let from = s.field_str("from").unwrap_or_default().to_string();
                let to = s.field_str("to").unwrap_or_default().to_string();
                for field in ["from", "to"] {
                    if let Some(value) = s.field_str(field) {
                        out.push(Reference {
                            site: Site::Connection { field, from: from.clone(), to: to.clone() },
                            path: value.to_string(),
                            bases: vec![".".to_string()],
                        });
                    }
                }
                // Bound arguments are resolved by the receiver, from the root.
                if let Some(binds) = s.field("binds") {
                    collect(&binds.value, &mut |path| {
                        out.push(Reference {
                            site: Site::Connection {
                                field: "binds",
                                from: from.clone(),
                                to: to.clone(),
                            },
                            path: path.to_string(),
                            bases: vec![".".to_string()],
                        })
                    });
                }
            }
            SectionKind::Editable => {
                if let Some(path) = s.field_str("path") {
                    out.push(Reference {
                        site: Site::Editable,
                        path: path.to_string(),
                        bases: vec![".".to_string()],
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn collect(value: &crate::value::Value, push: &mut dyn FnMut(&str)) {
    value.visit_node_paths(push);
}

/// The nodes a property on `section` might be measured from: the node itself,
/// and wherever its `root_node` points, which is what an `AnimationPlayer` or
/// `AnimationTree` resolves tracks and root motion against.
///
/// `default_hop` decides whether an absent `root_node` still means "my parent".
/// It does for a sub-resource, whose animation tracks resolve that way, and it
/// must not for an ordinary property: offering the parent as a base would make
/// every `../Thing` climb out of the file from there and be waved through,
/// which is exactly the reference a rename leaves behind.
fn node_bases(section: &Section, path: &str, default_hop: bool) -> Vec<String> {
    let mut bases = vec![path.to_string()];
    let root = match section.prop("root_node").and_then(path_of) {
        Some(explicit) => Some(explicit),
        None if default_hop => Some("..".to_string()),
        None => None,
    };
    if let Some(root) = root {
        if let Some(target) = walk(path, &root) {
            if !bases.contains(&target) {
                bases.push(target);
            }
        }
    }
    bases
}

/// The path a property holds, whether it is written `NodePath("x")` or `"x"`.
fn path_of(property: &crate::doc::Property) -> Option<String> {
    let mut found = None;
    property.value.visit_node_paths(&mut |p| {
        if found.is_none() {
            found = Some(p.to_string());
        }
    });
    found.or_else(|| property.value.as_str().map(str::to_string))
}

/// Applies a plain relative path to a node path, or `None` if it climbs out.
fn walk(from: &str, relative: &str) -> Option<String> {
    let mut parts = segments(from);
    for part in relative.split('/').filter(|p| !p.is_empty()) {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(join(&parts))
}

/// The nodes each sub-resource could be resolved against.
///
/// A sub-resource is not in the tree, so its paths are relative to whichever
/// node uses it, reached through however many other sub-resources. An
/// `AnimationPlayer` resolves its tracks against `root_node`, which defaults to
/// its own parent, so that node is offered as well. Both are accepted rather
/// than one being chosen, because guessing wrong would mean rejecting a valid
/// file.
fn sub_resource_bases(doc: &Document) -> HashMap<String, HashSet<String>> {
    let mut bases: HashMap<String, HashSet<String>> = HashMap::new();

    for s in doc.sections_of(SectionKind::Node) {
        let path = node_path(s);
        let candidates: HashSet<String> = node_bases(s, &path, true).into_iter().collect();
        for r in s.refs().filter(|r| r.kind == RefKind::Sub) {
            bases.entry(r.id.clone()).or_default().extend(candidates.iter().cloned());
        }
    }

    // A sub-resource used by another sub-resource inherits its bases.
    let subs: Vec<&Section> = doc.sections_of(SectionKind::SubResource).collect();
    for _ in 0..subs.len() + 1 {
        let mut changed = false;
        for s in &subs {
            let Some(id) = s.field_str("id") else { continue };
            let Some(mine) = bases.get(id).cloned() else { continue };
            for r in s.refs().filter(|r| r.kind == RefKind::Sub) {
                if r.id == id {
                    continue;
                }
                let entry = bases.entry(r.id.clone()).or_default();
                let before = entry.len();
                entry.extend(mine.iter().cloned());
                changed |= entry.len() != before;
            }
        }
        if !changed {
            break;
        }
    }
    bases
}

/// Every reference that did not resolve cleanly, with why.
///
/// `check` turns these into messages and `merge` inspects them, so the walking
/// happens once and both agree about what counts as broken.
pub(crate) fn findings(doc: &Document) -> Vec<(Reference, Resolution)> {
    let tree = Tree::new(doc);
    let mut out = Vec::new();
    for reference in references(doc) {
        let mut bases = reference.bases.clone();
        if root_is_plausible(&reference.path) && !bases.iter().any(|b| b == ".") {
            bases.push(".".to_string());
        }
        let mut worst: Option<Resolution> = None;
        let mut resolved = false;
        for base in &bases {
            match tree.resolve(base, &reference.path) {
                // Resolving from any one plausible base is enough.
                Resolution::Node(_) | Resolution::Outside => {
                    resolved = true;
                    break;
                }
                other => {
                    // A definite miss is more useful to report than a vague one,
                    // but anything uncertain takes precedence over it, because
                    // uncertainty is the reason not to call the file broken.
                    let keep = match (&worst, &other) {
                        (None, _) => true,
                        (Some(Resolution::Missing(_)), o)
                            if !matches!(o, Resolution::Missing(_)) =>
                        {
                            true
                        }
                        _ => false,
                    };
                    if keep {
                        worst = Some(other);
                    }
                }
            }
        }
        if resolved {
            continue;
        }
        if let Some(outcome) = worst {
            out.push((reference, outcome));
        }
    }
    out
}
