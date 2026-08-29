//! Validation of `NodePath` values, which is what stops a merge from wiring a
//! scene to a node that is not there.

use tscn::{Document, MergeOptions, Severity};

fn report(src: &str) -> (Vec<String>, Vec<String>) {
    let doc = Document::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"));
    let r = tscn::check(&doc, src);
    let errors = r
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.message.clone())
        .collect();
    let warnings = r
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .map(|i| i.message.clone())
        .collect();
    (errors, warnings)
}

fn errors(src: &str) -> Vec<String> {
    report(src).0
}

const HEADER: &str = "[gd_scene format=3 uid=\"uid://np\"]\n\n";

#[test]
fn a_path_that_names_a_real_node_is_accepted() {
    let (errors, warnings) = report(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         remote_path = NodePath(\"../Hero\")\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_path_that_names_nothing_is_an_error() {
    let errors = errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         remote_path = NodePath(\"../Hero\")\n"
    ));
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("remote_path on node \"Shadow\""), "{}", errors[0]);
    assert!(errors[0].contains("\"Hero\""), "{}", errors[0]);
}

#[test]
fn a_path_is_relative_to_the_node_that_holds_it() {
    // "Hero" from Wrapper means Wrapper/Hero, which does not exist, even though
    // a node called Hero does exist at the root.
    let errors = errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Wrapper\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Deep\" type=\"Node\" parent=\"Wrapper\"]\n\
         link = NodePath(\"../Hero\")\n"
    ));
    assert_eq!(errors.len(), 1, "expected the sibling lookup to fail: {errors:?}");
    assert!(errors[0].contains("\"Wrapper/Hero\""), "{}", errors[0]);
}

#[test]
fn a_subname_selects_a_property_and_does_not_affect_the_node_part() {
    assert!(errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Tween\" type=\"Node\" parent=\".\"]\n\
         watch = NodePath(\"../Hero:modulate:a\")\n"
    ))
    .is_empty());

    let broken = errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Tween\" type=\"Node\" parent=\".\"]\n\
         watch = NodePath(\"../Hero:modulate\")\n"
    ));
    assert_eq!(broken.len(), 1, "{broken:?}");
}

#[test]
fn a_leading_colon_means_the_holding_node_itself() {
    assert!(errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n\
         self_ref = NodePath(\":position\")\n"
    ))
    .is_empty());
}

#[test]
fn animation_track_paths_resolve_against_the_player() {
    let scene = |track: &str| {
        format!(
            "[gd_scene load_steps=3 format=3]\n\n\
             [sub_resource type=\"Animation\" id=\"A\"]\n\
             tracks/0/path = NodePath(\"{track}\")\n\n\
             [sub_resource type=\"AnimationLibrary\" id=\"L\"]\n\
             _data = {{\n&\"go\": SubResource(\"A\")\n}}\n\n\
             [node name=\"Root\" type=\"Node2D\"]\n\n\
             [node name=\"Hero\" type=\"Sprite2D\" parent=\".\"]\n\n\
             [node name=\"Anim\" type=\"AnimationPlayer\" parent=\".\"]\n\
             libraries/ = SubResource(\"L\")\n"
        )
    };
    // root_node defaults to the player's parent, so "Hero" is the root's child.
    assert!(errors(&scene("Hero:position")).is_empty(), "{:?}", errors(&scene("Hero:position")));
    let broken = errors(&scene("Ghost:position"));
    assert_eq!(broken.len(), 1, "{broken:?}");
    assert!(broken[0].contains("sub_resource"), "{}", broken[0]);
}

#[test]
fn an_explicit_root_node_moves_where_tracks_resolve_from() {
    let src = format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Model\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Bone\" type=\"Node2D\" parent=\"Model\"]\n\n\
         [node name=\"Tree\" type=\"AnimationTree\" parent=\".\"]\n\
         root_node = NodePath(\"../Model\")\n\
         root_motion_track = NodePath(\"Bone:position\")\n"
    );
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
}

#[test]
fn connection_endpoints_and_editable_paths_are_checked() {
    let errors = errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\n\
         [connection signal=\"pressed\" from=\"Gone\" to=\".\" method=\"_on\"]\n\n\
         [editable path=\"AlsoGone\"]\n"
    ));
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("connection")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("[editable]")), "{errors:?}");
}

#[test]
fn a_path_reaching_into_an_instanced_scene_is_only_a_warning() {
    let (errors, warnings) = report(
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"PackedScene\" uid=\"uid://e\" path=\"res://e.tscn\" id=\"1_e\"]\n\n\
         [node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Enemy\" parent=\".\" instance=ExtResource(\"1_e\")]\n\n\
         [node name=\"Watcher\" type=\"Node\" parent=\".\"]\n\
         look = NodePath(\"../Enemy/Sprite\")\n",
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("instanced scene"), "{}", warnings[0]);
}

#[test]
fn an_absolute_path_and_one_above_the_root_are_left_alone() {
    let (errors, warnings) = report(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\
         far = NodePath(\"/root/Main/Thing\")\n\
         up = NodePath(\"../Sibling\")\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_unique_name_resolves_through_the_node_that_declares_it() {
    assert!(errors(&format!(
        "{HEADER}[node name=\"Panel\" type=\"Control\"]\n\n\
         [node name=\"Spin\" type=\"VBoxContainer\" parent=\".\"]\n\
         unique_name_in_owner = true\n\n\
         [node name=\"Readout\" type=\"Label\" parent=\".\"]\n\
         source = NodePath(\"%Spin\")\n"
    ))
    .is_empty());
}

#[test]
fn an_undeclared_unique_name_is_an_error_only_when_nothing_could_supply_it() {
    let alone = errors(&format!(
        "{HEADER}[node name=\"Panel\" type=\"Control\"]\n\
         source = NodePath(\"%Missing\")\n"
    ));
    assert_eq!(alone.len(), 1, "{alone:?}");
    assert!(alone[0].contains("%Missing"), "{}", alone[0]);

    // With an instanced scene present, the name might come from there.
    let (errors, warnings) = report(
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"PackedScene\" uid=\"uid://e\" path=\"res://e.tscn\" id=\"1_e\"]\n\n\
         [node name=\"Panel\" type=\"Control\"]\n\
         source = NodePath(\"%Missing\")\n\n\
         [node name=\"Sub\" parent=\".\" instance=ExtResource(\"1_e\")]\n",
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[test]
fn paths_nested_inside_arrays_and_dictionaries_are_found() {
    let errors = errors(&format!(
        "{HEADER}[node name=\"Root\" type=\"Node2D\"]\n\
         list = [NodePath(\"Gone\"), {{\"k\": NodePath(\"AlsoGone\")}}]\n"
    ));
    assert_eq!(errors.len(), 2, "{errors:?}");
}

/// The reason all of this exists: a rename that leaves a reference behind now
/// stops the merge instead of producing a scene wired to nothing.
#[test]
fn a_rename_that_strands_a_reference_becomes_a_conflict() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n\
         speed = 100.0\n\n\
         [node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         remote_path = NodePath(\"../Hero\")\n"
    );
    let ours = base.replace("name=\"Hero\"", "name=\"Player\"");
    let theirs = base.replace("speed = 100.0", "speed = 250.0");

    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );
    assert!(!outcome.is_clean(), "should not merge silently:\n{}", outcome.text);
    let detail = &outcome.conflicts[0].detail;
    assert!(detail.contains("remote_path"), "{detail}");
    assert!(detail.contains("Hero"), "{detail}");
}
