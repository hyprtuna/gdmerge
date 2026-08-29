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

/// The reason all of this exists: a rename no longer strands the references to
/// the node it moved.
#[test]
fn a_rename_carries_its_references_with_it() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n\
         speed = 100.0\n\n\
         [node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         remote_path = NodePath(\"../Hero\")\n"
    );
    let ours = base.replace("name=\"Hero\"", "name=\"Player\"");
    let theirs = base.replace("speed = 100.0", "speed = 250.0");

    let merged = |a: &str, b: &str| {
        tscn::merge(
            &Document::parse(&base).unwrap(),
            &Document::parse(a).unwrap(),
            &Document::parse(b).unwrap(),
            &MergeOptions::default(),
        )
    };
    for (a, b) in [(&ours, &theirs), (&theirs, &ours)] {
        let outcome = merged(a, b);
        assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
        assert!(
            outcome.text.contains("remote_path = NodePath(\"../Player\")"),
            "the reference should follow the rename:\n{}",
            outcome.text
        );
        assert!(!outcome.text.contains("Hero"), "{}", outcome.text);
    }
}

/// A reference the merge genuinely breaks, by deleting what it named, still
/// stops the merge rather than producing a scene wired to nothing.
#[test]
fn deleting_a_node_another_branch_references_conflicts() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n"
    );
    let ours = format!("{HEADER}[node name=\"Level\" type=\"Node2D\"]\n");
    let theirs = format!(
        "{base}\n[node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         remote_path = NodePath(\"../Hero\")\n"
    );

    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );
    assert!(!outcome.is_clean(), "should not merge silently:\n{}", outcome.text);
    assert!(outcome.conflicts[0].detail.contains("Hero"), "{:?}", outcome.conflicts);
}

/// Breakage that was already in the ancestor is not this merge's doing, so the
/// library passes it through rather than raising a conflict over it. That is
/// as far as it goes: `gdmerge merge` validates its inputs before merging and
/// hands a file with a broken reference to git's text merge, which the CLI
/// tests pin. `check` reports the break either way.
#[test]
fn the_library_passes_an_inherited_broken_reference_through() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\
         stale = NodePath(\"Ghost\")\n\n\
         [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n"
    );
    let ours = base.replace(
        "[node name=\"Hero\"",
        "[node name=\"Extra\" type=\"Node\" parent=\".\"]\n\n[node name=\"Hero\"",
    );
    let theirs = format!("{base}\n[node name=\"Other\" type=\"Node\" parent=\".\"]\n");

    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    assert!(!errors(&outcome.text).is_empty(), "check should still report it");
}

/// Rewriting resolves against the tree rather than matching text, so a node
/// whose name is a prefix of another's is not caught up in its rename.
#[test]
fn a_rename_does_not_touch_a_similarly_named_node() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Player\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"PlayerCamera\" type=\"Camera2D\" parent=\".\"]\n"
    );
    let ours = base.replace("name=\"Player\" type=\"Node2D\"", "name=\"Hero\" type=\"Node2D\"");
    let theirs = format!(
        "{base}\n[node name=\"Watcher\" type=\"Node\" parent=\".\"]\n\
         a = NodePath(\"../Player\")\n\
         b = NodePath(\"../PlayerCamera\")\n"
    );

    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );
    assert!(outcome.is_clean(), "{:?}", outcome.conflicts);
    assert!(outcome.text.contains("a = NodePath(\"../Hero\")"), "{}", outcome.text);
    assert!(
        outcome.text.contains("b = NodePath(\"../PlayerCamera\")"),
        "the other node must be untouched:\n{}",
        outcome.text
    );
}

/// A path measured from two different nodes to two different targets cannot be
/// rewritten with certainty, so it is left as it is.
#[test]
fn an_ambiguous_path_is_left_alone() {
    // "Thing" resolves from the holder and from the root, to different nodes.
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Thing\" type=\"Node\" parent=\".\"]\n\n\
         [node name=\"Holder\" type=\"Node\" parent=\".\"]\n\n\
         [node name=\"Thing\" type=\"Node\" parent=\"Holder\"]\n"
    );
    let ours = base.replace(
        "[node name=\"Thing\" type=\"Node\" parent=\"Holder\"]",
        "[node name=\"Moved\" type=\"Node\" parent=\"Holder\"]",
    );
    let theirs = format!("{base}\npick = NodePath(\"Thing\")\n");
    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );
    // Whatever it decides, it must not silently point somewhere else.
    let kept = outcome.text.contains("NodePath(\"Thing\")");
    assert!(kept || !outcome.is_clean(), "either left alone or conflicted:\n{}", outcome.text);
}

/// A stranded path is something a person has to resolve, so it has to arrive
/// looking like a conflict: markers around the entity that holds the path, and
/// the two candidates laid out. Before 0.3.2 the merged text carried no markers
/// at all for this and the conflict came with no rows, so `gdmerge mergetool`
/// printed an empty table and only stderr said anything had gone wrong.
#[test]
fn a_stranded_path_is_marked_at_the_entity_holding_it() {
    let base = format!(
        "{HEADER}[node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n\n\
         [node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]\n\
         update_rotation = true\n"
    );
    // One branch removes the node, the other starts pointing at it. Each file
    // is sound on its own; only the merge of the two strands the path.
    let ours = base.replace("[node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n\n", "");
    let theirs = format!("{base}remote_path = NodePath(\"../Hero\")\n");

    let outcome = tscn::merge(
        &Document::parse(&base).unwrap(),
        &Document::parse(&ours).unwrap(),
        &Document::parse(&theirs).unwrap(),
        &MergeOptions::default(),
    );

    assert_eq!(outcome.conflicts.len(), 1, "{:?}", outcome.conflicts);
    let conflict = &outcome.conflicts[0];
    assert_eq!(conflict.entity, "remote_path on node \"Shadow\"");
    assert!(conflict.detail.contains("../Hero"), "{}", conflict.detail);

    // The markers wrap the node the path is written in, not the whole file.
    assert!(outcome.text.contains("<<<<<<< ours"), "no markers:\n{}", outcome.text);
    assert!(outcome.text.contains(">>>>>>> theirs"), "{}", outcome.text);
    assert!(
        outcome.text.contains("[node name=\"Level\" type=\"Node2D\"]\n\n<<<<<<<"),
        "{}",
        outcome.text
    );

    // And the two candidates are there to choose between, with the item that
    // has to be resolved marked even though only one side wrote it.
    assert_eq!(conflict.key.as_deref(), Some("remote_path"));
    let row = conflict
        .rows
        .iter()
        .find(|r| r.key == "remote_path")
        .expect("a row for the property holding the path");
    assert!(row.differs, "the stranded item must be the one marked to resolve");
    assert_eq!(row.ours, None);
    assert_eq!(row.theirs.as_deref(), Some("NodePath(\"../Hero\")"));
    // Context rows are kept, and not marked.
    assert!(conflict.rows.iter().any(|r| r.key == "update_rotation" && !r.differs));
}
