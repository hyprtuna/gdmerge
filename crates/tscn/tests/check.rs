//! Structural checks that are not about `NodePath` values, which have their
//! own file.

use tscn::{Document, Severity};

fn errors(src: &str) -> Vec<String> {
    let doc = Document::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"));
    tscn::check(&doc, src)
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.message.clone())
        .collect()
}

const HEADER: &str =
    "[gd_scene format=3 uid=\"uid://check\"]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n";

fn siblings(a: &str, b: &str) -> String {
    format!(
        "{HEADER}[node name=\"A\" type=\"Node\" parent=\".\" {a}]\n\n\
         [node name=\"B\" type=\"Node\" parent=\".\" {b}]\n"
    )
}

/// Godot writes the index quoted, like every other header field it reads back
/// as a string, so this is the spelling that has to be caught.
#[test]
fn colliding_quoted_sibling_indices_are_an_error() {
    let errors = errors(&siblings("index=\"0\"", "index=\"0\""));
    assert_eq!(errors, vec!["2 children of \".\" share index 0".to_string()]);
}

#[test]
fn colliding_bare_sibling_indices_are_an_error() {
    let errors = errors(&siblings("index=0", "index=0"));
    assert_eq!(errors, vec!["2 children of \".\" share index 0".to_string()]);
}

/// Godot's loader converts either spelling to the same number, so the two
/// forms of one index collide with each other as well.
#[test]
fn a_quoted_and_a_bare_index_collide_when_they_name_the_same_slot() {
    let errors = errors(&siblings("index=\"0\"", "index=0"));
    assert_eq!(errors, vec!["2 children of \".\" share index 0".to_string()]);
}

#[test]
fn distinct_sibling_indices_are_accepted() {
    assert!(errors(&siblings("index=\"0\"", "index=\"1\"")).is_empty());
}

#[test]
fn the_same_index_under_different_parents_is_accepted() {
    let src = format!(
        "{HEADER}[node name=\"A\" type=\"Node\" parent=\".\"]\n\n\
         [node name=\"B\" type=\"Node\" parent=\".\"]\n\n\
         [node name=\"C\" type=\"Node\" parent=\"A\" index=\"0\"]\n\n\
         [node name=\"D\" type=\"Node\" parent=\"B\" index=\"0\"]\n"
    );
    assert!(errors(&src).is_empty());
}
