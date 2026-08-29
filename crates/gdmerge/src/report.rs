//! Presenting a conflict as something a person can read.
//!
//! Conflict markers say *that* two branches disagreed. They do not say what
//! about, which in a scene file can mean scrolling past forty unchanged
//! properties to find the one that matters. Both renderings here put the two
//! sides next to each other and point at the rows that actually differ.

use std::fmt::Write as _;

use tscn::{Conflict, ConflictRow};

use crate::show::shown;

/// The full side-by-side table, for a terminal.
///
/// Names go through [`shown`] like values: a node name or a property key is
/// text a scene file may make as long as it likes, and a report that printed
/// one whole would be unbounded through its identities instead of its values.
pub fn table(conflicts: &[Conflict]) -> String {
    let mut out = String::new();
    for (i, c) in conflicts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "Conflict {} of {}: {}", i + 1, conflicts.len(), shown(&c.entity));
        let _ = writeln!(out, "  {}", shown(&c.detail));
        out.push('\n');
        render_rows(&mut out, &c.rows);
    }
    if conflicts.is_empty() {
        return out;
    }
    out.push('\n');
    out.push_str(
        "Rows marked with > are the ones to resolve. Edit the file, remove the conflict\n\
         markers, then stage it.\n",
    );
    out
}

/// A compact rendering for the merge driver, which writes to standard error
/// while git is running and should stay quiet about what already agrees.
pub fn plain(conflicts: &[Conflict]) -> String {
    let mut out = String::new();
    for c in conflicts {
        let _ = writeln!(out, "gdmerge: conflict in {} ({})", shown(&c.entity), shown(&c.detail));
        for row in c.rows.iter().filter(|r| r.differs) {
            let _ = writeln!(
                out,
                "gdmerge:   {}: ours {} / theirs {}",
                shown(&row.key),
                cell(&row.ours),
                cell(&row.theirs)
            );
        }
    }
    out
}

fn render_rows(out: &mut String, rows: &[ConflictRow]) {
    let headers = ["", "property", "base", "ours", "theirs"];
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            [
                if r.differs { ">".to_string() } else { String::new() },
                shown(&r.key),
                cell(&r.base),
                cell(&r.ours),
                cell(&r.theirs),
            ]
        })
        .collect();
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }

    let line = |out: &mut String, cols: &[String; 5]| {
        let mut parts = Vec::new();
        for (i, c) in cols.iter().enumerate() {
            let pad = widths[i] - c.chars().count();
            parts.push(format!("{c}{}", " ".repeat(pad)));
        }
        let _ = writeln!(out, "  {}", parts.join("  ").trim_end());
    };

    let header: [String; 5] = headers.map(str::to_string);
    line(out, &header);
    // The marker column has no heading, so it gets no rule either.
    let rule: [String; 5] =
        std::array::from_fn(|i| if i == 0 { String::new() } else { "-".repeat(widths[i]) });
    line(out, &rule);
    for row in &cells {
        line(out, row);
    }
}

/// One value as a report shows it: bounded, on one line, an id always whole.
/// Absence is explicit, because "the node is not on this side" is the whole
/// story of a delete against a modify.
fn cell(value: &Option<String>) -> String {
    value.as_deref().map(shown).unwrap_or_else(|| "(absent)".to_string())
}
