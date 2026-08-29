//! The hard gate: every real Godot file in `tests/fixtures/` must survive a
//! parse/serialize round trip byte for byte, and must pass the structural
//! checks. These files come from godotengine/godot-demo-projects — see
//! `fixtures/ATTRIBUTION.md`.

use std::path::{Path, PathBuf};

use tscn::Document;

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("tscn") | Some("tres")))
        .collect();
    out.sort();
    assert!(out.len() >= 20, "expected at least 20 fixtures, found {}", out.len());
    out
}

#[test]
fn every_fixture_round_trips_byte_for_byte() {
    for path in fixtures() {
        let src = std::fs::read_to_string(&path).expect("fixture is UTF-8");
        let doc = Document::parse(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let out = doc.to_source();
        if out != src {
            let at = out
                .bytes()
                .zip(src.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| src.len().min(out.len()));
            panic!(
                "{} differs at byte {at}\n  expected: {:?}\n  produced: {:?}",
                path.display(),
                &src[at.saturating_sub(60)..(at + 60).min(src.len())],
                &out[at.saturating_sub(60)..(at + 60).min(out.len())],
            );
        }
    }
}
