//! Structural sanity checks: the same invariants Godot's loader relies on.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::doc::{Document, SectionKind};
use crate::scene::node_path;
use crate::value::RefKind;

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

    let mut ext_ids: HashSet<&str> = HashSet::new();
    let mut sub_ids: HashSet<&str> = HashSet::new();
    let (mut ext_count, mut sub_count) = (0usize, 0usize);

    for s in &doc.sections {
        match s.kind {
            SectionKind::ExtResource => {
                ext_count += 1;
                match s.field_str("id") {
                    Some(id) if !ext_ids.insert(id) => {
                        r.error(format!("duplicate ext_resource id \"{id}\""));
                    }
                    None => r.error("[ext_resource] has no id".to_string()),
                    _ => {}
                }
                if s.field_str("path").is_none() {
                    r.error(format!(
                        "[ext_resource id=\"{}\"] has no path",
                        s.field_str("id").unwrap_or("?")
                    ));
                }
            }
            SectionKind::SubResource => {
                sub_count += 1;
                match s.field_str("id") {
                    Some(id) if !sub_ids.insert(id) => {
                        r.error(format!("duplicate sub_resource id \"{id}\""));
                    }
                    None => r.error("[sub_resource] has no id".to_string()),
                    _ => {}
                }
                if s.field_str("type").is_none() {
                    r.error(format!(
                        "[sub_resource id=\"{}\"] has no type",
                        s.field_str("id").unwrap_or("?")
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
        for reference in s.refs() {
            let known = match reference.kind {
                RefKind::Ext => ext_ids.contains(reference.id.as_str()),
                RefKind::Sub => sub_ids.contains(reference.id.as_str()),
            };
            if !known {
                r.error(format!(
                    "dangling {}(\"{}\") reference",
                    reference.kind.ctor(),
                    reference.id
                ));
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
    r
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
