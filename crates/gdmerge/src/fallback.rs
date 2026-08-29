//! Text fallback for files gdmerge cannot understand.
//!
//! The safety contract is that gdmerge never does worse than git would. When a
//! file fails to parse — a truncated scene, a format from a future engine — the
//! merge is handed to `git merge-file`, and its exit status is passed through
//! unchanged.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use similar::{ChangeTag, TextDiff};

pub struct Fallback {
    pub text: String,
    /// Number of conflicted hunks; zero means git merged it cleanly.
    pub conflicts: usize,
}

/// Runs `git merge-file -p`, falling back to an in-process diff3 if git is not
/// on PATH.
pub fn merge_file(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    marker_size: usize,
    ours_label: &str,
    theirs_label: &str,
) -> Result<Fallback> {
    let out = Command::new("git")
        .arg("merge-file")
        .arg("-p")
        .arg("--diff3")
        .arg(format!("--marker-size={marker_size}"))
        .arg("-L")
        .arg(ours_label)
        .arg("-L")
        .arg("base")
        .arg("-L")
        .arg(theirs_label)
        .arg(ours)
        .arg(base)
        .arg(theirs)
        .output();

    match out {
        Ok(out) if out.status.code().is_some_and(|c| c >= 0) && out.status.code() != Some(127) => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            // git merge-file exits with the number of conflicts, or <0 on error.
            let conflicts = out.status.code().unwrap_or(0).max(0) as usize;
            Ok(Fallback { text, conflicts })
        }
        _ => line_merge(base, ours, theirs, marker_size, ours_label, theirs_label),
    }
}

/// Minimal three-way line merge, used only when `git` is unavailable.
fn line_merge(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    marker_size: usize,
    ours_label: &str,
    theirs_label: &str,
) -> Result<Fallback> {
    let b = std::fs::read_to_string(base).with_context(|| format!("reading {}", base.display()))?;
    let o = std::fs::read_to_string(ours).with_context(|| format!("reading {}", ours.display()))?;
    let t =
        std::fs::read_to_string(theirs).with_context(|| format!("reading {}", theirs.display()))?;

    if o == t {
        return Ok(Fallback { text: o, conflicts: 0 });
    }
    if b == o {
        return Ok(Fallback { text: t, conflicts: 0 });
    }
    if b == t {
        return Ok(Fallback { text: o, conflicts: 0 });
    }

    let changed_ours = changed_lines(&b, &o);
    let changed_theirs = changed_lines(&b, &t);
    if changed_ours == 0 || changed_theirs == 0 {
        let text = if changed_ours == 0 { t } else { o };
        return Ok(Fallback { text, conflicts: 0 });
    }

    let mut text = String::new();
    text.push_str(&format!("{} {ours_label}\n", "<".repeat(marker_size)));
    text.push_str(&o);
    if !o.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{}\n", "=".repeat(marker_size)));
    text.push_str(&t);
    if !t.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{} {theirs_label}\n", ">".repeat(marker_size)));
    Ok(Fallback { text, conflicts: 1 })
}

fn changed_lines(a: &str, b: &str) -> usize {
    TextDiff::from_lines(a, b).iter_all_changes().filter(|c| c.tag() != ChangeTag::Equal).count()
}
