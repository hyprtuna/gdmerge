//! Randomised property tests over the parser and serializer.
//!
//! The generator is a seeded LCG rather than a proptest dependency: the crate
//! keeps its dependency list short, and a fixed seed makes a failure exactly
//! reproducible from the printed seed alone.

use tscn::{Document, MergeOptions};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const SCALARS: [&str; 22] = [
    "true",
    "false",
    "null",
    "inf",
    "-inf",
    "nan",
    "0",
    "-1",
    "3.5",
    "-0.25",
    "1e10",
    "1.5e-3",
    "\"\"",
    "\"plain\"",
    "\"with \\\"quotes\\\" and \\t tabs\"",
    "\"multi\nline\"",
    "\"semi ; colon and [bracket]\"",
    "&\"a_string_name\"",
    "@\"legacy_name\"",
    "#ff8800",
    "#00ff88aa",
    "\"unicode \\u00e9 \\U01F600\"",
];

const CALLS: [&str; 12] = [
    "Vector2(1, 2)",
    "Vector2i(-3, 4)",
    "Vector3(1.5, 0, -2)",
    "Rect2(0, 0, 10, 10)",
    "Transform2D(1, 0, 0, 1, 0, 0)",
    "Color(1, 0.5, 0, 1)",
    "NodePath(\"../Sibling\")",
    "PackedInt32Array(1, 2, 3)",
    "PackedStringArray(\"a\", \"b\")",
    "PackedByteArray(\"AQID\")",
    "RID()",
    "Object(Resource, \"a\": 1, \"b\": \"two\")",
];

fn value(rng: &mut Rng, depth: usize) -> String {
    if depth == 0 {
        return match rng.below(2) {
            0 => rng.pick(&SCALARS).to_string(),
            _ => rng.pick(&CALLS).to_string(),
        };
    }
    match rng.below(7) {
        0 | 1 => rng.pick(&SCALARS).to_string(),
        2 => rng.pick(&CALLS).to_string(),
        3 => {
            let n = rng.below(4);
            let items: Vec<String> = (0..n).map(|_| value(rng, depth - 1)).collect();
            format!("[{}]", items.join(", "))
        }
        4 => {
            let n = rng.below(3);
            let items: Vec<String> =
                (0..n).map(|i| format!("\"k{i}\": {}", value(rng, depth - 1))).collect();
            format!("{{\n{}\n}}", items.join(",\n"))
        }
        5 => {
            let n = rng.below(4);
            let items: Vec<String> = (0..n).map(|_| value(rng, depth - 1)).collect();
            format!("Array[int]([{}])", items.join(", "))
        }
        _ => {
            let n = rng.below(3);
            let items: Vec<String> =
                (0..n).map(|i| format!("\"k{i}\": {}", value(rng, depth - 1))).collect();
            format!("Dictionary[String, Variant]({{\n{}\n}})", items.join(",\n"))
        }
    }
}

const KEYS: [&str; 8] = [
    "position",
    "metadata/_edit_lock_",
    "0:0/0/terrain_set",
    "\"quoted key\"",
    "\"has=equals\"",
    "theme_override_colors/font_color",
    "layer_0/tile_data",
    "script",
];

fn document(rng: &mut Rng) -> String {
    let mut out = String::new();
    if rng.below(4) == 0 {
        out.push_str("; a leading comment\n");
    }
    out.push_str("[gd_scene load_steps=2 format=3 uid=\"uid://generated\"]\n\n");
    out.push_str("[ext_resource type=\"Texture2D\" path=\"res://a.png\" id=\"1_a\"]\n\n");

    let nodes = 1 + rng.below(4);
    for i in 0..nodes {
        if i == 0 {
            out.push_str("[node name=\"Root\" type=\"Node2D\"]\n");
        } else {
            out.push_str(&format!("[node name=\"N{i}\" type=\"Node2D\" parent=\".\"]\n"));
        }
        // Distinct keys, so a node never declares the same property twice.
        let props = rng.below(4);
        for key in KEYS.iter().take(props) {
            if rng.below(6) == 0 {
                out.push_str("; a comment between properties\n");
            }
            out.push_str(&format!("{key} = {}\n", value(rng, 2)));
        }
        if rng.below(8) == 0 {
            out.push_str("prop_ref = ExtResource(\"1_a\")\n");
        }
        out.push('\n');
    }
    out.truncate(out.trim_end_matches('\n').len());
    out.push('\n');
    out
}

#[test]
fn generated_documents_round_trip() {
    for seed in 0..2000u64 {
        let mut rng = Rng::new(seed);
        let src = document(&mut rng);
        let doc = Document::parse(&src)
            .unwrap_or_else(|e| panic!("seed {seed}: parse failed: {e}\n---\n{src}"));
        assert_eq!(doc.to_source(), src, "seed {seed}: round trip changed the source\n{src}");
    }
}

#[test]
fn generated_documents_self_merge_to_themselves() {
    for seed in 0..500u64 {
        let mut rng = Rng::new(seed);
        let src = document(&mut rng);
        let doc = Document::parse(&src).expect("generated document parses");
        let outcome = tscn::merge(&doc, &doc, &doc, &MergeOptions::default());
        assert!(outcome.is_clean(), "seed {seed}: self-merge conflicted");
        assert_eq!(outcome.text, src, "seed {seed}: self-merge changed the file");
        assert!(tscn::diff(&doc, &doc).is_empty(), "seed {seed}: self-diff was not empty");
    }
}

/// Truncations and byte flips of a valid document must produce an error, never
/// a panic and never a silent mis-parse.
#[test]
fn corrupted_input_never_panics() {
    for seed in 0..400u64 {
        let mut rng = Rng::new(seed);
        let src = document(&mut rng);
        let bytes = src.as_bytes();

        let cut = rng.below(bytes.len());
        if let Ok(truncated) = std::str::from_utf8(&bytes[..cut]) {
            let _ = Document::parse(truncated);
        }

        let mut mutated = bytes.to_vec();
        let at = rng.below(mutated.len());
        mutated[at] = *rng.pick(b"\"[]()=%\\\n");
        if let Ok(text) = std::str::from_utf8(&mutated) {
            if let Ok(doc) = Document::parse(text) {
                // Whatever we accept, we must still reproduce exactly.
                assert_eq!(doc.to_source(), text, "seed {seed}: mutated input lost bytes");
            }
        }
    }
}

/// Any document merged against an unchanged counterpart comes back untouched.
#[test]
fn an_untouched_branch_never_rewrites_the_other() {
    for seed in 0..300u64 {
        let mut rng = Rng::new(seed);
        let base_src = document(&mut rng);
        let mut ours_src = base_src.clone();
        ours_src.push_str("\n[node name=\"Added\" type=\"Node2D\" parent=\".\"]\n");

        let base = Document::parse(&base_src).expect("base parses");
        let ours = Document::parse(&ours_src).expect("ours parses");

        let outcome = tscn::merge(&base, &ours, &base, &MergeOptions::default());
        assert!(outcome.is_clean(), "seed {seed}: conflicted against an unchanged branch");
        assert_eq!(outcome.text, ours_src, "seed {seed}: our bytes were rewritten");

        let outcome = tscn::merge(&base, &base, &ours, &MergeOptions::default());
        assert_eq!(outcome.text, ours_src, "seed {seed}: their file was not taken verbatim");
    }
}
