//! Structural sanity checks: the same invariants Godot's loader relies on.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::doc::{Document, SectionKind};
use crate::nodepath::{findings, Resolution};
use crate::scene::node_path;
use crate::value::{id_text, is_legacy_id, RefKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Godot would fail to load the file, or load it wrongly.
    Error,
    /// Harmless but wrong, such as a stale `load_steps`.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckIssue {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct CheckReport {
    pub issues: Vec<CheckIssue>,
}

impl CheckReport {
    pub fn errors(&self) -> impl Iterator<Item = &CheckIssue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }

    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    fn error(&mut self, message: String) {
        self.issues.push(CheckIssue { severity: Severity::Error, message });
    }

    fn warn(&mut self, message: String) {
        self.issues.push(CheckIssue { severity: Severity::Warning, message });
    }
}

/// Runs every structural check over a parsed document.
///
/// `source` is the original text; it is re-serialised to prove the round trip is
/// lossless, which is the invariant everything else here depends on.
pub fn check(doc: &Document, source: &str) -> CheckReport {
    let mut r = CheckReport::default();

    if doc.to_source() != source {
        r.error("re-serialising the parsed file does not reproduce it byte for byte".to_string());
    }

    let mut ext_ids: HashSet<String> = HashSet::new();
    let mut sub_ids: HashSet<String> = HashSet::new();
    let (mut ext_count, mut sub_count) = (0usize, 0usize);

    for s in &doc.sections {
        match s.kind {
            SectionKind::ExtResource => {
                ext_count += 1;
                let id = declared_id(s, "ext_resource", &mut ext_ids, &mut r);
                if s.field_str("path").is_none() {
                    r.error(format!(
                        "[ext_resource id=\"{}\"] has no path",
                        id.as_deref().unwrap_or("?")
                    ));
                }
            }
            SectionKind::SubResource => {
                sub_count += 1;
                let id = declared_id(s, "sub_resource", &mut sub_ids, &mut r);
                if s.field_str("type").is_none() {
                    r.error(format!(
                        "[sub_resource id=\"{}\"] has no type",
                        id.as_deref().unwrap_or("?")
                    ));
                }
            }
            SectionKind::Node if s.field_str("name").is_none() => {
                r.error("[node] has no name".to_string());
            }
            SectionKind::Connection => {
                for field in ["signal", "from", "to", "method"] {
                    if s.field_str(field).is_none() {
                        r.error(format!("[connection] is missing '{field}'"));
                    }
                }
            }
            _ => {}
        }
    }

    for s in &doc.sections {
        for (kind, id) in s.all_refs() {
            let known = match kind {
                RefKind::Ext => ext_ids.contains(&id),
                RefKind::Sub => sub_ids.contains(&id),
            };
            if !known {
                r.error(format!("dangling {}(\"{id}\") reference", kind.ctor()));
            }
        }
    }

    // `load_steps` is a loader progress hint. Godot omits it entirely in many
    // saved files, so only a *present* value is checked.
    if let Some(field) = doc.header().field("load_steps") {
        if let Some(declared) = field.value.as_num() {
            let expected = (ext_count + sub_count + 1) as f64;
            if declared != expected {
                r.warn(format!("load_steps is {declared:.0} but should be {expected:.0}"));
            }
        }
    }

    check_nodes(doc, &mut r);
    check_node_paths(doc, &mut r);
    r
}

/// Records the id a resource section declares, reporting what is wrong with it.
///
/// Godot 4 quotes resource ids. Godot 3 wrote them as bare integers, and a file
/// that still does is not one gdmerge can merge: the id has no stable spelling
/// to renumber, and the `SubResource( 1 )` references to it are written the same
/// legacy way. Saying so here is what keeps `merge` from touching such a file.
fn declared_id(
    s: &crate::doc::Section,
    tag: &str,
    seen: &mut HashSet<String>,
    r: &mut CheckReport,
) -> Option<String> {
    let Some(field) = s.field("id") else {
        r.error(format!("[{tag}] has no id"));
        return None;
    };
    let Some(id) = id_text(&field.value) else {
        r.error(format!("[{tag}] has an id that is neither a string nor a number"));
        return None;
    };
    if is_legacy_id(&field.value) {
        r.error(format!(
            "[{tag} id={id}] uses the Godot 3 unquoted id form; Godot 4 quotes resource ids"
        ));
    }
    if !seen.insert(id.clone()) {
        r.error(format!("duplicate {tag} id \"{id}\""));
    }
    Some(id)
}

/// Every `NodePath` in the file has to name something.
///
/// A path that definitely resolves to nothing is an error: it is a scene wired
/// to a node that is not there, which loads fine and then does nothing. Paths
/// that leave the file, reach into an instanced scene, or use a unique name
/// this file does not declare cannot be judged from here, so they are reported
/// as warnings or not at all rather than guessed at.
fn check_node_paths(doc: &Document, r: &mut CheckReport) {
    for (reference, outcome) in findings(doc) {
        match outcome {
            Resolution::Missing(target) => r.error(format!(
                "NodePath(\"{}\") in {} points at \"{}\", which is not a node in this file",
                reference.path,
                reference.site.describe(),
                target
            )),
            Resolution::Instanced => r.warn(format!(
                "NodePath(\"{}\") in {} reaches into an instanced scene and cannot be checked here",
                reference.path,
                reference.site.describe()
            )),
            // A file with no instanced scenes has nowhere else for a unique
            // name to come from, so an unknown one is definitely broken.
            Resolution::UnknownUniqueName { name, supplied_elsewhere: false } => r.error(format!(
                "NodePath(\"{}\") in {} uses the unique name \"%{}\", which no node in this file declares",
                reference.path,
                reference.site.describe(),
                name
            )),
            Resolution::UnknownUniqueName { name, .. } => r.warn(format!(
                "NodePath(\"{}\") in {} uses the unique name \"%{}\", which this file does not declare; an instanced scene may supply it",
                reference.path,
                reference.site.describe(),
                name
            )),
            Resolution::Node(_) | Resolution::Outside => {}
        }
    }
}

fn check_nodes(doc: &Document, r: &mut CheckReport) {
    let nodes: Vec<_> = doc.sections_of(SectionKind::Node).collect();
    let mut paths: HashSet<String> = HashSet::new();
    let mut instanced: Vec<String> = Vec::new();
    let mut roots = 0usize;

    for s in &nodes {
        let path = node_path(s);
        if s.field("parent").is_none() {
            roots += 1;
        }
        if !paths.insert(path.clone()) {
            r.error(format!("duplicate node path \"{path}\""));
        }
        if s.field("instance").is_some() || s.field("instance_placeholder").is_some() {
            instanced.push(path);
        }
    }

    if !nodes.is_empty() && roots == 0 {
        r.error("scene has no root node (every [node] declares a parent)".to_string());
    }
    if roots > 1 {
        r.error(format!("scene has {roots} root nodes; exactly one is allowed"));
    }

    for s in &nodes {
        let Some(parent) = s.field_str("parent") else { continue };
        if parent == "." || paths.contains(parent) {
            continue;
        }
        // A node may override something that lives inside an instanced scene, in
        // which case the parent legitimately is not declared in this file. That
        // holds for any ancestor being instanced, the root included.
        if covered_by_instance(parent, &instanced) {
            continue;
        }
        r.error(format!(
            "node \"{}\" has parent \"{parent}\", which is not a node in this file",
            node_path(s)
        ));
    }

    let mut index_dupes: HashMap<(&str, i64), usize> = HashMap::new();
    for s in &nodes {
        let (Some(parent), Some(index)) =
            (s.field_str("parent"), s.field("index").and_then(|f| f.value.as_num()))
        else {
            continue;
        };
        *index_dupes.entry((parent, index as i64)).or_insert(0) += 1;
    }
    for ((parent, index), n) in index_dupes {
        if n > 1 {
            r.error(format!("{n} children of \"{parent}\" share index {index}"));
        }
    }
}

/// True when `parent` sits inside a scene instanced by one of `instanced`.
///
/// An instanced node's children are defined by the scene it instances, not by
/// this file, so an undeclared parent under one of them is expected. When the
/// *root* is an instance (path `.`), every path in the file is covered.
fn covered_by_instance(parent: &str, instanced: &[String]) -> bool {
    instanced.iter().any(|i| {
        i == "."
            || parent == i
            || parent.starts_with(&format!("{i}/"))
            // Or the instance is itself deeper than `parent` is long: the parent
            // is an ancestor path that the instance's own scene provides.
            || i.starts_with(&format!("{parent}/"))
    })
}
