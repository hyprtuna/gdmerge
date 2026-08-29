//! Golden three-way merge cases.
//!
//! Each directory under `merge_cases/` holds `base`, `ours` and `theirs`, plus
//! the `expected` result and, when the merge is meant to conflict, a
//! `conflicts.txt` listing one entity description per line.
//!
//! Re-record the expected files after an intentional behaviour change with
//! `GDMERGE_BLESS=1 cargo test -p tscn --test merge_golden`, then read the diff.

use std::path::{Path, PathBuf};

use tscn::{Document, MergeOptions};

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/merge_cases")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

struct Case {
    name: String,
    dir: PathBuf,
    ext: &'static str,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(cases_dir()).expect("merge_cases directory").flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let ext = if dir.join("base.tscn").exists() { "tscn" } else { "tres" };
        assert!(
            dir.join(format!("base.{ext}")).exists(),
            "{} has no base.tscn or base.tres",
            dir.display()
        );
        out.push(Case { name: dir.file_name().unwrap().to_string_lossy().into_owned(), dir, ext });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(out.len() >= 15, "expected at least 15 golden cases, found {}", out.len());
    out
}

#[test]
fn golden_merges_match() {
    let bless = std::env::var_os("GDMERGE_BLESS").is_some();
    let mut failures = Vec::new();

    for case in cases() {
        let base_src = read(&case.dir.join(format!("base.{}", case.ext)));
        let ours_src = read(&case.dir.join(format!("ours.{}", case.ext)));
        let theirs_src = read(&case.dir.join(format!("theirs.{}", case.ext)));

        let base = Document::parse(&base_src).expect("base parses");
        let ours = Document::parse(&ours_src).expect("ours parses");
        let theirs = Document::parse(&theirs_src).expect("theirs parses");

        let outcome = tscn::merge(&base, &ours, &theirs, &MergeOptions::default());
        let conflicts: Vec<String> = outcome.conflicts.iter().map(|c| c.entity.clone()).collect();

        let expected_path = case.dir.join(format!("expected.{}", case.ext));
        let conflicts_path = case.dir.join("conflicts.txt");

        if bless {
            std::fs::write(&expected_path, &outcome.text).expect("writing expected");
            if conflicts.is_empty() {
                let _ = std::fs::remove_file(&conflicts_path);
            } else {
                std::fs::write(&conflicts_path, format!("{}\n", conflicts.join("\n")))
                    .expect("writing conflicts");
            }
            continue;
        }

        let expected = read(&expected_path);
        if outcome.text != expected {
            failures.push(format!(
                "{}: merged output differs from expected.{}\n--- expected ---\n{expected}\n--- actual ---\n{}",
                case.name, case.ext, outcome.text
            ));
        }

        let expected_conflicts: Vec<String> = std::fs::read_to_string(&conflicts_path)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        if conflicts != expected_conflicts {
            failures.push(format!(
                "{}: conflicts {conflicts:?} != expected {expected_conflicts:?}",
                case.name
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Whatever the merge produces must itself be a file gdmerge can read back, and
/// a clean merge must additionally be structurally sound.
#[test]
fn merged_output_is_well_formed() {
    for case in cases() {
        let base_src = read(&case.dir.join(format!("base.{}", case.ext)));
        let ours_src = read(&case.dir.join(format!("ours.{}", case.ext)));
        let theirs_src = read(&case.dir.join(format!("theirs.{}", case.ext)));
        let base = Document::parse(&base_src).unwrap();
        let ours = Document::parse(&ours_src).unwrap();
        let theirs = Document::parse(&theirs_src).unwrap();

        let outcome = tscn::merge(&base, &ours, &theirs, &MergeOptions::default());
        if !outcome.is_clean() {
            continue; // Conflict markers are not valid scene syntax by design.
        }
        let doc = Document::parse(&outcome.text)
            .unwrap_or_else(|e| panic!("{}: merged output does not parse: {e}", case.name));
        let report = tscn::check(&doc, &outcome.text);
        let errors: Vec<_> = report.errors().map(|i| i.message.clone()).collect();
        assert!(errors.is_empty(), "{}: merged output fails check: {errors:?}", case.name);
    }
}

/// Merging is symmetric in the sense that swapping the sides must not change
/// *whether* the merge is clean, only which side wins a tie.
#[test]
fn cleanliness_is_side_independent() {
    for case in cases() {
        let base_src = read(&case.dir.join(format!("base.{}", case.ext)));
        let ours_src = read(&case.dir.join(format!("ours.{}", case.ext)));
        let theirs_src = read(&case.dir.join(format!("theirs.{}", case.ext)));
        let base = Document::parse(&base_src).unwrap();
        let ours = Document::parse(&ours_src).unwrap();
        let theirs = Document::parse(&theirs_src).unwrap();

        let forward = tscn::merge(&base, &ours, &theirs, &MergeOptions::default());
        let reverse = tscn::merge(&base, &theirs, &ours, &MergeOptions::default());
        assert_eq!(
            forward.is_clean(),
            reverse.is_clean(),
            "{}: swapping ours/theirs changed whether the merge is clean",
            case.name
        );
        assert_eq!(
            forward.conflicts.len(),
            reverse.conflicts.len(),
            "{}: swapping ours/theirs changed the conflict count",
            case.name
        );
    }
}

/// A branch that changed nothing must never disturb the other branch's bytes.
#[test]
fn unchanged_side_preserves_the_other_verbatim() {
    for case in cases() {
        let base_src = read(&case.dir.join(format!("base.{}", case.ext)));
        let ours_src = read(&case.dir.join(format!("ours.{}", case.ext)));
        let base = Document::parse(&base_src).unwrap();
        let ours = Document::parse(&ours_src).unwrap();

        let outcome = tscn::merge(&base, &ours, &base, &MergeOptions::default());
        assert!(outcome.is_clean(), "{}: merging against an unchanged side conflicted", case.name);
        assert_eq!(outcome.text, ours_src, "{}: an unchanged 'theirs' rewrote our file", case.name);

        let outcome = tscn::merge(&base, &base, &ours, &MergeOptions::default());
        assert_eq!(
            outcome.text, ours_src,
            "{}: an unchanged 'ours' failed to take their file verbatim",
            case.name
        );
    }
}

/// A CRLF file must come back entirely CRLF. Sections are replayed from their
/// original bytes, but the blank lines a merge inserts between them are
/// synthesised, and a Windows checkout with `core.autocrlf=true` hands gdmerge
/// CRLF files and expects CRLF back.
#[test]
fn crlf_documents_stay_crlf() {
    fn to_crlf(s: &str) -> String {
        s.replace("\r\n", "\n").replace('\n', "\r\n")
    }

    for case in cases() {
        let base_src = to_crlf(&read(&case.dir.join(format!("base.{}", case.ext))));
        let ours_src = to_crlf(&read(&case.dir.join(format!("ours.{}", case.ext))));
        let theirs_src = to_crlf(&read(&case.dir.join(format!("theirs.{}", case.ext))));
        let base = Document::parse(&base_src).expect("base parses");
        let ours = Document::parse(&ours_src).expect("ours parses");
        let theirs = Document::parse(&theirs_src).expect("theirs parses");

        let outcome = tscn::merge(&base, &ours, &theirs, &MergeOptions::default());
        let lone_lf =
            outcome.text.as_bytes().windows(2).filter(|w| w[1] == b'\n' && w[0] != b'\r').count();
        assert_eq!(lone_lf, 0, "{}: merged output mixes CRLF and LF:\n{}", case.name, outcome.text);

        // The expected output is the recorded one with its line endings swapped.
        let expected = to_crlf(&read(&case.dir.join(format!("expected.{}", case.ext))));
        assert_eq!(outcome.text, expected, "{}: CRLF merge differs from the LF merge", case.name);
    }
}

/// Merging the same three files twice has to give the same answer.
///
/// Several stages key entities by hash, and Rust seeds each map differently, so
/// anything that iterates one and keeps the first result can silently depend on
/// that seed. A merge tool that is not deterministic is not trustworthy.
#[test]
fn merging_is_deterministic() {
    for case in cases() {
        let base_src = read(&case.dir.join(format!("base.{}", case.ext)));
        let ours_src = read(&case.dir.join(format!("ours.{}", case.ext)));
        let theirs_src = read(&case.dir.join(format!("theirs.{}", case.ext)));

        let mut first: Option<(String, Vec<String>)> = None;
        for attempt in 0..24 {
            // Parsed afresh each time so every run builds new hash maps.
            let base = Document::parse(&base_src).unwrap();
            let ours = Document::parse(&ours_src).unwrap();
            let theirs = Document::parse(&theirs_src).unwrap();
            let outcome = tscn::merge(&base, &ours, &theirs, &MergeOptions::default());
            let conflicts: Vec<String> =
                outcome.conflicts.iter().map(|c| format!("{}: {}", c.entity, c.detail)).collect();
            match &first {
                None => first = Some((outcome.text, conflicts)),
                Some((text, expected)) => {
                    assert_eq!(
                        &outcome.text, text,
                        "{}: attempt {attempt} produced different output",
                        case.name
                    );
                    assert_eq!(
                        &conflicts, expected,
                        "{}: attempt {attempt} produced different conflicts",
                        case.name
                    );
                }
            }
        }
    }
}
