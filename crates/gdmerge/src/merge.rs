//! `gdmerge merge`

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use tscn::{Document, MergeOptions};

use crate::{fallback, io, report, MergeArgs, EXIT_CONFLICT};

/// Where the merged text has to end up.
enum Destination {
    Stdout,
    File(PathBuf),
}

pub fn run(args: MergeArgs) -> Result<i32> {
    let (base, ours, theirs, dest, marker_size, pathname) = resolve_inputs(&args)?;

    // Under the driver the three inputs are git's temporary files, whose
    // names mean nothing to anyone; git passes the scene's own path as %P for
    // exactly this, so that is what a person is told.
    let name = |path: &Path, version: &str| match &pathname {
        Some(scene) => format!("{scene}, {version} version"),
        None => path.display().to_string(),
    };

    let opts = MergeOptions {
        ours_label: args.ours_label.clone(),
        theirs_label: args.theirs_label.clone(),
        marker_size,
    };

    let (base_src, ours_src, theirs_src) = (io::read(&base)?, io::read(&ours)?, io::read(&theirs)?);

    let text_merge = || -> Result<i32> {
        let r = fallback::merge_file(
            &base,
            &ours,
            &theirs,
            marker_size,
            &args.ours_label,
            &args.theirs_label,
        )?;
        emit(&dest, &r.text)?;
        Ok(if r.conflicts > 0 { EXIT_CONFLICT } else { 0 })
    };

    // Anything we cannot parse is handed straight to git's text merge, and its
    // exit status is what the caller sees.
    let parsed =
        (Document::parse(&base_src), Document::parse(&ours_src), Document::parse(&theirs_src));
    let (b, o, t) = match parsed {
        (Ok(b), Ok(o), Ok(t)) => (b, o, t),
        (b, o, t) => {
            let why =
                [(&base, "base", b.err()), (&ours, "ours", o.err()), (&theirs, "theirs", t.err())]
                    .into_iter()
                    .find_map(|(path, version, e)| {
                        e.map(|e| format!("{}: {e}", name(path, version)))
                    })
                    .unwrap_or_default();
            eprintln!("gdmerge: falling back to a text merge ({why})");
            return text_merge();
        }
    };

    // Parsing is not enough. A file that parses but does not hold together is
    // one the semantic merge cannot reason about safely, so it gets the same
    // treatment as one that does not parse at all.
    let invalid = [
        (&base, "base", &b, &base_src),
        (&ours, "ours", &o, &ours_src),
        (&theirs, "theirs", &t, &theirs_src),
    ]
    .into_iter()
    .find_map(|(path, version, doc, src)| {
        fallback::structural_error(doc, src).map(|why| (path, version, why))
    });
    if let Some((path, version, why)) = invalid {
        eprintln!("gdmerge: falling back to a text merge ({}: {why})", name(path, version));
        return text_merge();
    }

    let outcome = tscn::merge(&b, &o, &t, &opts);

    // A merged file that no longer parses would be worse than a conflict, so
    // prove the output is well-formed before it is allowed anywhere near disk.
    if outcome.is_clean() {
        if let Err(e) = Document::parse(&outcome.text) {
            eprintln!(
                "gdmerge: merged output failed self-validation ({e}); falling back to a text merge"
            );
            return text_merge();
        }
    }

    emit(&dest, &outcome.text)?;
    if !outcome.is_clean() {
        eprint!("{}", report::plain(&outcome.conflicts));
        eprintln!("gdmerge: run `git mergetool --tool=gdmerge` to see the two sides side by side");
    }
    Ok(if outcome.is_clean() { 0 } else { EXIT_CONFLICT })
}

fn emit(dest: &Destination, text: &str) -> Result<()> {
    match dest {
        Destination::Stdout => {
            print!("{text}");
            Ok(())
        }
        Destination::File(p) => io::write_atomic(p, text),
    }
}

/// Accepts both the flag form and git's positional merge-driver form.
///
/// git invokes the driver as `%O %A %B %L %P`: ancestor, ours, theirs, conflict
/// marker size, and the path being merged. The result must be left in `%A`.
/// The last item is `%P` when it was given: the name to show for the file.
type Inputs = (PathBuf, PathBuf, PathBuf, Destination, usize, Option<String>);

fn resolve_inputs(args: &MergeArgs) -> Result<Inputs> {
    if let (Some(base), Some(ours), Some(theirs)) = (&args.base, &args.ours, &args.theirs) {
        if !args.positional.is_empty() {
            bail!("pass either --base/--ours/--theirs or git's positional form, not both");
        }
        let dest = match &args.output {
            Some(p) => Destination::File(p.clone()),
            None => Destination::Stdout,
        };
        return Ok((base.clone(), ours.clone(), theirs.clone(), dest, args.marker_size, None));
    }
    if args.base.is_some() || args.ours.is_some() || args.theirs.is_some() {
        bail!("--base, --ours and --theirs must be given together");
    }

    let p = &args.positional;
    if p.len() < 3 {
        bail!(
            "expected --base/--ours/--theirs, or git's positional form: \
             gdmerge merge %O %A %B [%L] [%P]"
        );
    }
    let base = PathBuf::from(&p[0]);
    let ours = PathBuf::from(&p[1]);
    let theirs = PathBuf::from(&p[2]);
    // %L is a number; %P is a path. Either, both, or neither may follow.
    let (marker_size, pathname) = match p.get(3).map(|s| s.parse::<usize>()) {
        Some(Ok(size)) => (size, p.get(4).cloned()),
        Some(Err(_)) => (args.marker_size, p.get(3).cloned()),
        None => (args.marker_size, None),
    };
    let dest = match &args.output {
        Some(path) => Destination::File(path.clone()),
        // git expects the driver to overwrite %A in place.
        None => Destination::File(ours.clone()),
    };
    ensure_exists(&base)?;
    ensure_exists(&ours)?;
    ensure_exists(&theirs)?;
    Ok((base, ours, theirs, dest, marker_size, pathname))
}

fn ensure_exists(p: &Path) -> Result<()> {
    if !p.exists() {
        bail!("{} does not exist", p.display());
    }
    Ok(())
}
