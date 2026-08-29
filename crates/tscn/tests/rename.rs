//! Renames and reparents, in the diff and through the merge.

use tscn::{Change, Document, MergeOptions};

fn doc(src: &str) -> Document {
    Document::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"))
}

fn merged(base: &str, ours: &str, theirs: &str) -> tscn::MergeOutcome {
    tscn::merge(&doc(base), &doc(ours), &doc(theirs), &MergeOptions::default())
}

const BASE: &str = "\
[gd_scene format=3 uid=\"uid://rename\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]
speed = 100.0
";

const RENAMED: &str = "\
[gd_scene format=3 uid=\"uid://rename\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Player\" type=\"CharacterBody2D\" parent=\".\"]
speed = 100.0
";

const EDITED: &str = "\
[gd_scene format=3 uid=\"uid://rename\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]
speed = 250.0
";

#[test]
fn the_diff_reports_a_rename_as_a_move() {
    let d = tscn::diff(&doc(BASE), &doc(RENAMED));
    let moves: Vec<_> = d
        .changes
        .iter()
        .filter_map(|c| match c {
            Change::Moved { from, to, .. } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(moves, vec![("node Hero".to_string(), "node Player".to_string())]);
}

#[test]
fn a_rename_merges_with_an_edit_from_the_other_branch() {
    let outcome = merged(BASE, RENAMED, EDITED);
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    assert!(outcome.text.contains("name=\"Player\""), "{}", outcome.text);
    assert!(outcome.text.contains("speed = 250.0"), "{}", outcome.text);
    assert!(!outcome.text.contains("Hero"), "the old name should be gone:\n{}", outcome.text);
}

/// The same merge with the sides swapped has to reach the same file.
#[test]
fn rename_and_edit_is_order_independent() {
    let forward = merged(BASE, RENAMED, EDITED);
    let reverse = merged(BASE, EDITED, RENAMED);
    assert!(reverse.is_clean(), "{:?}", reverse.conflicts);
    assert_eq!(forward.text, reverse.text);
}

#[test]
fn a_reparent_merges_with_an_edit_from_the_other_branch() {
    let base = "\
[gd_scene format=3 uid=\"uid://reparent\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Holder\" type=\"Node2D\" parent=\".\"]

[node name=\"Item\" type=\"Sprite2D\" parent=\".\"]
offset = Vector2(1, 1)
";
    let ours = base.replace(
        "name=\"Item\" type=\"Sprite2D\" parent=\".\"",
        "name=\"Item\" type=\"Sprite2D\" parent=\"Holder\"",
    );
    let theirs = base.replace("offset = Vector2(1, 1)", "offset = Vector2(8, 8)");

    let outcome = merged(base, &ours, &theirs);
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    assert!(outcome.text.contains("parent=\"Holder\""), "{}", outcome.text);
    assert!(outcome.text.contains("offset = Vector2(8, 8)"), "{}", outcome.text);
}

#[test]
fn renaming_to_two_different_names_conflicts() {
    let theirs = RENAMED.replace("Player", "Protagonist");
    let outcome = merged(BASE, RENAMED, &theirs);
    assert!(!outcome.is_clean());
    assert_eq!(outcome.conflicts.len(), 1);
    assert_eq!(outcome.conflicts[0].detail, "renamed differently on both sides");
    assert!(outcome.text.contains("Player"), "{}", outcome.text);
    assert!(outcome.text.contains("Protagonist"), "{}", outcome.text);
}

/// Renaming a node moves everything under it, and everything that names it.
#[test]
fn renaming_a_parent_carries_its_subtree() {
    let base = "\
[gd_scene format=3 uid=\"uid://subtree\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Holder\" type=\"Node2D\" parent=\".\"]

[node name=\"Item\" type=\"Sprite2D\" parent=\"Holder\"]
offset = Vector2(1, 1)
";
    let ours = base
        .replace("name=\"Holder\"", "name=\"Box\"")
        .replace("parent=\"Holder\"", "parent=\"Box\"");
    let theirs = format!(
        "{base}\n[node name=\"Label\" type=\"Label\" parent=\"Holder\"]\ntext = \"hi\"\n\n\
         [connection signal=\"pressed\" from=\"Holder\" to=\".\" method=\"_on_pressed\"]\n\n\
         [editable path=\"Holder\"]\n"
    );

    let outcome = merged(base, &ours, &theirs);
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    // Everything that pointed at Holder now points at Box.
    assert!(!outcome.text.contains("Holder"), "a stale path survived:\n{}", outcome.text);
    assert!(
        outcome.text.contains("[node name=\"Label\" type=\"Label\" parent=\"Box\"]"),
        "{}",
        outcome.text
    );
    assert!(outcome.text.contains("from=\"Box\""), "{}", outcome.text);
    assert!(outcome.text.contains("[editable path=\"Box\"]"), "{}", outcome.text);

    // And the result is still a structurally sound scene.
    let parsed = Document::parse(&outcome.text).expect("merged output parses");
    let report = tscn::check(&parsed, &outcome.text);
    let errors: Vec<_> = report.errors().map(|i| i.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// Pairing is deliberately conservative: a branch that renames and edits in one
/// step is not matched, and the merge reports the delete against the modify
/// rather than guessing. This test pins that limitation so a future change to
/// it is a deliberate one.
#[test]
fn a_rename_combined_with_an_edit_is_not_matched() {
    let ours = RENAMED.replace("speed = 100.0", "speed = 175.0");
    let outcome = merged(BASE, &ours, EDITED);
    assert!(!outcome.is_clean());
    assert!(
        outcome.conflicts.iter().any(|c| c.detail.contains("deleted by ours")),
        "{:?}",
        outcome.conflicts
    );
}

/// A node added under a subtree the other branch renamed still lands in the
/// right place, which is what keeps the merged file loadable.
#[test]
fn an_added_child_follows_a_renamed_parent() {
    let base = "\
[gd_scene format=3 uid=\"uid://added\"]

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Holder\" type=\"Node2D\" parent=\".\"]
";
    let ours = base.replace("name=\"Holder\"", "name=\"Box\"");
    let theirs = format!("{base}\n[node name=\"New\" type=\"Node2D\" parent=\"Holder\"]\n");

    let outcome = merged(base, &ours, &theirs);
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    assert!(outcome.text.contains("parent=\"Box\""), "{}", outcome.text);
    let parsed = Document::parse(&outcome.text).expect("merged output parses");
    assert!(!tscn::check(&parsed, &outcome.text).has_errors());
}
