//! Grammar coverage for shapes that are rare enough to be missing from the
//! fixture corpus but are all accepted by Godot's own `VariantParser`.

use tscn::{Document, Value};

fn round_trips(src: &str) -> Document {
    let doc = Document::parse(src).unwrap_or_else(|e| panic!("parsing failed: {e}\n{src}"));
    assert_eq!(doc.to_source(), src, "round trip changed the source");
    doc
}

#[test]
fn object_literals() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         thing = Object(Resource, \"script\": null, \"nested\": Object(Resource, \"a\": 1))\n",
    );
    let value = &doc.sections[1].prop("thing").unwrap().value;
    let Value::Object { ty, props } = value else { panic!("expected an Object, got {value:?}") };
    assert_eq!(ty, "Resource");
    assert_eq!(props.len(), 2);
    assert_eq!(props[0].0, "script");
}

#[test]
fn object_literal_with_no_properties() {
    round_trips("[gd_resource type=\"Resource\" format=3]\n\n[resource]\nx = Object(Resource,)\n");
}

#[test]
fn line_comments_are_preserved() {
    round_trips(
        "; leading comment\n[gd_scene format=3]\n\n\
         ; before the node\n[node name=\"Root\" type=\"Node\"]\n\
         ; before a property\nvalue = 1\n; trailing comment\n",
    );
}

#[test]
fn semicolons_inside_strings_are_not_comments() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\ntext = \"a ; b\"\n",
    );
    assert_eq!(doc.sections[1].prop("text").unwrap().value.as_str(), Some("a ; b"));
}

#[test]
fn brackets_inside_strings_do_not_start_a_section() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"RichTextLabel\"]\n\
         text = \"[b]bold[/b]\n[node name=\\\"fake\\\"]\"\n",
    );
    assert_eq!(doc.sections.len(), 2, "a string must not be mistaken for a section");
}

#[test]
fn multi_line_strings() {
    let doc = round_trips(
        "[gd_resource type=\"Shader\" format=3]\n\n[resource]\n\
         code = \"shader_type canvas_item;\n\nvoid fragment() {\n\tCOLOR = vec4(1.0);\n}\"\n",
    );
    let code = doc.sections[1].prop("code").unwrap().value.as_str().unwrap();
    assert!(code.contains("void fragment"));
    assert_eq!(code.lines().count(), 5);
}

#[test]
fn string_escapes() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         a = \"tab\\there\"\nb = \"quote\\\"inside\"\nc = \"\\u00e9\"\nd = \"\\U01F600\"\n",
    );
    let s = &doc.sections[1];
    assert_eq!(s.prop("a").unwrap().value.as_str(), Some("tab\there"));
    assert_eq!(s.prop("b").unwrap().value.as_str(), Some("quote\"inside"));
    assert_eq!(s.prop("c").unwrap().value.as_str(), Some("é"));
    assert_eq!(s.prop("d").unwrap().value.as_str(), Some("😀"));
}

#[test]
fn surrogate_pairs_recombine() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\nx = \"\\ud83d\\ude00\"\n",
    );
    assert_eq!(doc.sections[1].prop("x").unwrap().value.as_str(), Some("😀"));
}

#[test]
fn negative_infinity_is_an_identifier() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         lo = -inf\nhi = inf\nnope = nan\n",
    );
    let s = &doc.sections[1];
    assert_eq!(s.prop("lo").unwrap().value, Value::Ident("-inf".into()));
    assert_eq!(s.prop("hi").unwrap().value, Value::Ident("inf".into()));
    assert_eq!(s.prop("nope").unwrap().value, Value::Ident("nan".into()));
}

#[test]
fn quoted_property_names() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         \"has=equals\" = 1\n\"has space\" = 2\nmetadata/_edit_lock_ = true\n",
    );
    let s = &doc.sections[1];
    assert!(s.prop("has=equals").is_some(), "a quoted name may contain '='");
    assert!(s.prop("has space").is_some());
    assert!(s.prop("metadata/_edit_lock_").is_some());
}

#[test]
fn property_names_with_punctuation() {
    let doc = round_trips(
        "[gd_resource type=\"TileSet\" format=3]\n\n[resource]\n\
         0:0/0 = 0\n0:0/0/terrain_set = -1\nsources/0 = 1\n",
    );
    let s = &doc.sections[1];
    assert!(s.prop("0:0/0").is_some());
    assert!(s.prop("0:0/0/terrain_set").is_some());
}

#[test]
fn typed_arrays_and_dictionaries() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         a = Array[int]([1, 2, 3])\n\
         b = Array[Resource]([])\n\
         c = Dictionary[String, Vector2]({\n\"k\": Vector2(1, 2)\n})\n",
    );
    let s = &doc.sections[1];
    assert!(matches!(s.prop("a").unwrap().value, Value::TypedArray { .. }));
    assert!(matches!(s.prop("c").unwrap().value, Value::TypedDict { .. }));
}

#[test]
fn string_names_and_colours() {
    let doc = round_trips(
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\
         n = &\"walk\"\nold = @\"run\"\nc = #ff8800\nc2 = #ff8800aa\n",
    );
    let s = &doc.sections[1];
    assert_eq!(s.prop("n").unwrap().value, Value::Name("walk".into()));
    assert_eq!(s.prop("old").unwrap().value, Value::Name("run".into()));
    assert_eq!(s.prop("c").unwrap().value, Value::Color("ff8800".into()));
    assert_eq!(s.prop("c2").unwrap().value, Value::Color("ff8800aa".into()));
}

#[test]
fn node_header_fields() {
    let doc = round_trips(
        "[gd_scene load_steps=2 format=3 uid=\"uid://abc\"]\n\n\
         [ext_resource type=\"PackedScene\" uid=\"uid://s\" path=\"res://s.tscn\" id=\"1_a\"]\n\n\
         [node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Ghost\" parent=\".\" instance_placeholder=\"res://s.tscn\"]\n\n\
         [node name=\"Real\" parent=\".\" owner=\"..\" index=\"3\" \
         groups=[\"a\", \"b\"] instance=ExtResource(\"1_a\")]\n\n\
         [connection signal=\"hit\" from=\"Real\" to=\".\" method=\"_on_hit\" \
         flags=3 unbinds=1 binds= [1, \"x\"]]\n\n\
         [editable path=\"Real\"]\n",
    );
    let real = doc.sections.iter().find(|s| s.field_str("name") == Some("Real")).unwrap();
    assert_eq!(real.field_str("owner"), Some(".."));
    assert_eq!(real.field_str("index"), Some("3"));
    assert!(real.field("instance").is_some());
    let conn = doc.sections.iter().find(|s| s.tag == "connection").unwrap();
    assert!(conn.field("binds").is_some());
    assert!(conn.field("flags").is_some());
}

#[test]
fn crlf_line_endings_survive() {
    round_trips("[gd_scene format=3]\r\n\r\n[node name=\"Root\" type=\"Node\"]\r\nx = 1\r\n");
}

#[test]
fn a_file_without_a_trailing_newline_survives() {
    round_trips("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\nx = 1");
}

#[test]
fn resource_references_are_located_precisely() {
    let doc = round_trips(
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Texture2D\" path=\"res://a.png\" id=\"1_a\"]\n\n\
         [node name=\"Root\" type=\"Sprite2D\"]\n\
         nested = [ExtResource(\"1_a\"), {\"k\": SubResource(\"S_1\")}]\n",
    );
    let prop = doc.sections[2].prop("nested").unwrap();
    assert_eq!(prop.refs.len(), 2);
    for r in &prop.refs {
        let slice = &prop.value_raw[r.span.clone()];
        assert_eq!(slice, format!("\"{}\"", r.id), "span must cover the quoted id");
    }
}

#[test]
fn rejects_files_that_are_not_godot_text_resources() {
    assert!(Document::parse("hello world\n").is_err());
    assert!(Document::parse("[node name=\"Root\"]\n").is_err());
    assert!(Document::parse("").is_err());
}

#[test]
fn reports_the_line_of_a_syntax_error() {
    let err = Document::parse("[gd_scene format=3]\n\n[node name=\"Root\"]\nx = Vector2(1, 2\n")
        .unwrap_err();
    assert_eq!(err.line, 5, "error should point at the unterminated call");
}

/// Nesting is parsed recursively, so an unbounded depth means a file with a few
/// thousand opening brackets aborts the process instead of failing to parse.
#[test]
fn deeply_nested_values_are_rejected_not_fatal() {
    for depth in [200usize, 5_000, 200_000] {
        let src = format!(
            "[gd_scene format=3]\n\n[node name=\"R\" type=\"Node\"]\nx = {}{}\n",
            "[".repeat(depth),
            "]".repeat(depth)
        );
        let err = Document::parse(&src).expect_err("should be refused, not accepted");
        assert!(
            matches!(err.kind, tscn::ParseErrorKind::ValueTooDeep(_)),
            "depth {depth} gave {err}"
        );
    }
}

/// The limit has to be well clear of anything a real scene contains.
#[test]
fn ordinary_nesting_is_still_accepted() {
    let src = format!(
        "[gd_scene format=3]\n\n[node name=\"R\" type=\"Node\"]\nx = {}{}\n",
        "[".repeat(100),
        "]".repeat(100)
    );
    round_trips(&src);
}
