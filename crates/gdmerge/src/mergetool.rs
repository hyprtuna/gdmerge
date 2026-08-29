//! `gdmerge mergetool`
//!
//! What `git mergetool --tool=gdmerge` runs. It redoes the semantic merge from
//! the three pristine versions git hands it, writes the result over the
//! conflicted file, and prints what the two branches disagreed about.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tscn::{Document, MergeOptions};

use crate::{fallback, io, report, EXIT_CONFLICT};

pub fn run(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    merged: Option<&PathBuf>,
    opts: &MergeOptions,
) -> Result<i32> {
    let destination = merged.map(PathBuf::as_path).unwrap_or(ours);

    let (base_src, ours_src, theirs_src) = (io::read(base)?, io::read(ours)?, io::read(theirs)?);
    let parsed =
        (Document::parse(&base_src), Document::parse(&ours_src), Document::parse(&theirs_src));
    let text_merge = || -> Result<i32> {
        let r = fallback::merge_file(
            base,
            ours,
            theirs,
            opts.marker_size,
            &opts.ours_label,
            &opts.theirs_label,
        )?;
        io::write_atomic(destination, &r.text)?;
        Ok(if r.conflicts > 0 { EXIT_CONFLICT } else { 0 })
    };

    let (b, o, t) = match parsed {
        (Ok(b), Ok(o), Ok(t)) => (b, o, t),
        (b, o, t) => {
            let why = [(base, b.err()), (ours, o.err()), (theirs, t.err())]
                .into_iter()
                .find_map(|(path, e)| e.map(|e| format!("{}: {e}", path.display())))
                .unwrap_or_default();
            println!("gdmerge cannot read one of these files ({why}).");
            println!("Falling back to a text merge; resolve it by hand.");
            return text_merge();
        }
    };

    // The mergetool redoes the merge and overwrites the conflicted file, so it
    // needs the same guard `merge` has: an input the semantic merge cannot be
    // trusted with goes to git's text merge here too.
    let invalid = [(base, &b, &base_src), (ours, &o, &ours_src), (theirs, &t, &theirs_src)]
        .into_iter()
        .find_map(|(path, doc, src)| fallback::structural_error(doc, src).map(|why| (path, why)));
    if let Some((path, why)) = invalid {
        println!("gdmerge cannot merge these files safely ({}: {why}).", path.display());
        println!("Falling back to a text merge; resolve it by hand.");
        return text_merge();
    }

    let outcome = tscn::merge(&b, &o, &t, opts);
    io::write_atomic(destination, &outcome.text)?;

    if outcome.is_clean() {
        println!("gdmerge merged {} with no conflicts.", destination.display());
        return Ok(0);
    }

    let n = outcome.conflicts.len();
    println!("{n} conflict{} in {}\n", if n == 1 { "" } else { "s" }, destination.display());
    print!("{}", report::table(&outcome.conflicts));
    Ok(EXIT_CONFLICT)
}
