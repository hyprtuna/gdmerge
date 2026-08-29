//! The golden merge corpus, run through the binary.
//!
//! `crates/tscn/tests/merge_golden.rs` holds every case to the library's
//! answer. The binary stands in front of the library with a gate: an input
//! that fails `check` is sent to a text merge instead, and a case whose
//! inputs trip that gate would record an answer nobody can reach from the
//! command line. So every case is also run here, and has to give the same
//! output, the same conflicts and the same exit status from the outside.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_gdmerge");

/// Cases the binary is expected to refuse. Each needs a reason: a case whose
/// inputs the gate turns away records library-only behaviour, and the
/// default is to rebuild it with inputs that pass. None at the moment.
const LIBRARY_ONLY: &[(&str, &str)] = &[];

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tscn/tests/merge_cases")
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
        out.push(Case { name: dir.file_name().unwrap().to_string_lossy().into_owned(), dir, ext });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(out.len() >= 15, "expected at least 15 golden cases, found {}", out.len());
    out
}

/// The entities the driver's report names, in the order it names them.
fn reported_conflicts(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|l| l.strip_prefix("gdmerge: conflict in "))
        .map(|l| l.rsplit_once(" (").map(|(entity, _)| entity).unwrap_or(l).to_string())
        .collect()
}

#[test]
fn every_golden_case_gives_the_same_answer_through_the_binary() {
    if std::env::var_os("GDMERGE_BLESS").is_some() {
        return; // The expected files are being rewritten by the library test.
    }
    let mut failures = Vec::new();

    for case in cases() {
        let file = |stem: &str| case.dir.join(format!("{stem}.{}", case.ext));
        let out = Command::new(BIN)
            .arg("merge")
            .args(["-b".as_ref(), file("base").as_os_str()])
            .args(["-o".as_ref(), file("ours").as_os_str()])
            .args(["-t".as_ref(), file("theirs").as_os_str()])
            .output()
            .expect("running gdmerge");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);

        if let Some((_, why)) = LIBRARY_ONLY.iter().find(|(name, _)| *name == case.name) {
            assert!(
                stderr.contains("falling back to a text merge"),
                "{}: listed as library-only ({why}) but the binary merged it semantically",
                case.name
            );
            continue;
        }
        if stderr.contains("falling back") {
            failures.push(format!("{}: the binary refused an input:\n{stderr}", case.name));
            continue;
        }

        let expected = read(&file("expected"));
        if stdout != expected {
            failures.push(format!(
                "{}: the binary's output differs from expected.{}\n--- expected ---\n{expected}\n--- actual ---\n{stdout}",
                case.name, case.ext
            ));
        }

        let expected_conflicts: Vec<String> =
            std::fs::read_to_string(case.dir.join("conflicts.txt"))
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
        let conflicts = reported_conflicts(&stderr);
        if conflicts != expected_conflicts {
            failures.push(format!(
                "{}: the binary reported {conflicts:?}, expected {expected_conflicts:?}\n{stderr}",
                case.name
            ));
        }

        let want = if expected_conflicts.is_empty() { 0 } else { 1 };
        if out.status.code() != Some(want) {
            failures.push(format!("{}: exit status {:?}, expected {want}", case.name, out.status));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The corpus is also checked from the outside: every input the binary is
/// asked to merge semantically has to be one `gdmerge check` accepts, or the
/// test above is not testing what it says.
#[test]
fn every_golden_input_passes_check() {
    let mut failures = Vec::new();
    for case in cases() {
        if LIBRARY_ONLY.iter().any(|(name, _)| *name == case.name) {
            continue;
        }
        for stem in ["base", "ours", "theirs"] {
            let path = case.dir.join(format!("{stem}.{}", case.ext));
            let out = Command::new(BIN).arg("check").arg(&path).output().expect("running gdmerge");
            if !out.status.success() {
                failures.push(format!(
                    "{}/{stem}: {}",
                    case.name,
                    String::from_utf8_lossy(&out.stdout).trim()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
