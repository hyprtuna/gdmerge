//! A guard against the parser or the merge becoming accidentally quadratic.
//!
//! This is not a benchmark. The bound is deliberately far above what the work
//! actually takes, so it stays quiet on a slow or loaded machine and only fires
//! when something has changed by an order of magnitude.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tscn::{Document, MergeOptions};

/// Generous on purpose: the work below takes single digit milliseconds on a
/// developer machine, and CI runners are slower and share a host.
const BUDGET: Duration = Duration::from_secs(10);

fn largest_fixture() -> (PathBuf, String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut biggest: Option<(PathBuf, String)> = None;
    for entry in std::fs::read_dir(&dir).expect("fixtures directory").flatten() {
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("tscn") | Some("tres")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        // `map_or` rather than `is_none_or`, which postdates this crate's MSRV.
        if biggest.as_ref().map_or(true, |(_, current)| text.len() > current.len()) {
            biggest = Some((path, text));
        }
    }
    biggest.expect("at least one fixture")
}

#[test]
fn the_largest_fixture_parses_and_merges_quickly() {
    let (path, source) = largest_fixture();
    assert!(
        source.len() > 20_000,
        "{} is smaller than expected, so this guards less than it should",
        path.display()
    );

    let started = Instant::now();
    for _ in 0..10 {
        let doc = Document::parse(&source).expect("fixture parses");
        assert_eq!(doc.to_source(), source);
        // Merging a file with itself exercises identity, diff and emission over
        // every entity in the file, which is where the cost would appear.
        let outcome = tscn::merge(&doc, &doc, &doc, &MergeOptions::default());
        assert!(outcome.is_clean());
        let report = tscn::check(&doc, &source);
        assert!(!report.has_errors());
    }
    let taken = started.elapsed();

    assert!(
        taken < BUDGET,
        "ten parse, round trip, merge and check passes over {} ({} bytes) took {taken:?}, \
         which is past the {BUDGET:?} budget",
        path.display(),
        source.len()
    );
    eprintln!(
        "{} ({} bytes): ten passes in {taken:?}",
        path.file_name().unwrap().to_string_lossy(),
        source.len()
    );
}
