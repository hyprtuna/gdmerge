//! Structural checks that are not about `NodePath` values, which have their
//! own file.

use tscn::{Document, Severity};

fn issues(src: &str, severity: Severity) -> Vec<String> {
    let doc = Document::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"));
    tscn::check(&doc, src)
        .issues
        .iter()
        .filter(|i| i.severity == severity)
        .map(|i| i.message.clone())
        .collect()
}

fn errors(src: &str) -> Vec<String> {
    issues(src, Severity::Error)
}

fn warnings(src: &str) -> Vec<String> {
    issues(src, Severity::Warning)
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

/// Godot converts the text with `String::to_int`, under which `"abc"` is 0, so
/// two such siblings collide with each other and with a real `"0"`.
#[test]
fn sibling_indices_that_godot_reads_as_the_same_number_collide() {
    let expected = vec!["2 children of \".\" share index 0".to_string()];
    assert_eq!(errors(&siblings("index=\"abc\"", "index=\"abc\"")), expected);
    assert_eq!(errors(&siblings("index=\"abc\"", "index=\"0\"")), expected);
}

#[test]
fn quoted_indices_that_read_as_different_numbers_are_accepted() {
    assert!(errors(&siblings("index=\"-1\"", "index=\"1\"")).is_empty());
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

/// `load_steps` is read the same way as `index`, so the quoted spelling is
/// checked for staleness like the bare one.
#[test]
fn a_quoted_load_steps_is_checked_for_staleness() {
    let src = "[gd_scene load_steps=\"99\" format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n";
    assert_eq!(warnings(src), vec!["load_steps is 99 but should be 1".to_string()]);
    assert!(errors(src).is_empty());
}

#[test]
fn a_quoted_load_steps_that_is_right_is_accepted() {
    let src = "[gd_scene load_steps=\"1\" format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n";
    assert!(warnings(src).is_empty(), "{:?}", warnings(src));
    assert!(errors(src).is_empty());
}

const SHAPE: &str = "[gd_scene load_steps=2 format=3]\n\n\
                     [sub_resource type=\"RectangleShape2D\" id=\"1\"]\n\
                     size = Vector2(1, 1)\n\n\
                     [node name=\"Root\" type=\"CollisionShape2D\"]\n";

/// A reference written the Godot 3 way is one the merge cannot renumber, even
/// when the declaration it points at is quoted, so it is an error on its own.
#[test]
fn a_legacy_reference_to_a_quoted_declaration_is_an_error() {
    let errors = errors(&format!("{SHAPE}shape = SubResource( 1 )\n"));
    assert_eq!(
        errors,
        vec!["SubResource(1) uses the Godot 3 unquoted id form; Godot 4 quotes resource ids"
            .to_string()]
    );
}

#[test]
fn a_quoted_reference_to_a_quoted_declaration_is_accepted() {
    assert!(errors(&format!("{SHAPE}shape = SubResource(\"1\")\n")).is_empty());
}
