//! `gdmerge git-install` / `gdmerge git-uninstall`
//!
//! Two things have to be in place for git to use a custom merge driver: the
//! driver definition in a config file, and an attributes entry that points the
//! relevant paths at it. Both are written here so setup is a single command.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const DRIVER: &str = "gdmerge";
const NAME: &str = "Godot 4 scene and resource merge";
/// The driver git runs, as a shell fragment: git invokes a merge driver through
/// the shell, so the command can decide for itself what to do.
///
/// It has to, because a driver git cannot execute is worse than no driver at
/// all. git leaves `%A` holding our side untouched and reports the file
/// conflicted, with no markers in it and the other side nowhere; `git add` then
/// throws that side away silently. Checking for the binary first and otherwise
/// running git's own text merge, whose exit status passes straight through,
/// makes the worst case the merge you would have had without gdmerge
/// installed.
const COMMAND: &str = "if command -v gdmerge >/dev/null 2>&1; then \
                       gdmerge merge %O %A %B %L %P; else \
                       git merge-file -L ours -L base -L theirs %A %O %B; fi";
// git runs a mergetool command through the shell with these variables set.
const MERGETOOL_COMMAND: &str = "gdmerge mergetool \"$BASE\" \"$LOCAL\" \"$REMOTE\" \"$MERGED\"";
const MARKER: &str = "# gdmerge";
/// Appended to the marker when `add_attributes` had to end the file's last
/// line before writing its block, so `remove_attributes` can take that newline
/// away again and hand back the file byte for byte.
const MARKER_NOTE: &str = " (the line above had no newline)";
const PATTERNS: [&str; 2] = ["*.tscn", "*.tres"];

pub fn install(global: bool) -> Result<i32> {
    let scope = Scope::detect(global)?;
    set(&scope, &format!("merge.{DRIVER}.name"), NAME)?;
    set(&scope, &format!("merge.{DRIVER}.driver"), COMMAND)?;
    // Tells git how to merge the virtual ancestor of a criss-cross merge: keep
    // one side rather than attempting a text merge of two scene files.
    set(&scope, &format!("merge.{DRIVER}.recursive"), "binary")?;

    // The mergetool is separate from the driver: the driver resolves what it
    // can during `git merge`, the mergetool explains what it could not.
    set(&scope, &format!("mergetool.{DRIVER}.cmd"), MERGETOOL_COMMAND)?;
    set(&scope, &format!("mergetool.{DRIVER}.trustExitCode"), "true")?;

    let attributes = scope.attributes_path()?;
    let added = add_attributes(&attributes)?;

    println!("configured the '{DRIVER}' merge driver and mergetool in {}", scope.describe());
    if added {
        println!("added *.tscn and *.tres rules to {}", attributes.display());
    } else {
        println!("{} already had gdmerge rules", attributes.display());
    }
    println!("\nwhen a merge does conflict, `git mergetool --tool={DRIVER}` shows the two sides");
    println!("side by side. To make it the default: git config merge.tool {DRIVER}");
    if !global {
        println!("\ncommit .gitattributes so the whole team gets the same behaviour;");
        println!("each teammate still runs `gdmerge git-install` once to define the driver.");
    }
    Ok(0)
}

pub fn uninstall(global: bool) -> Result<i32> {
    let scope = Scope::detect(global)?;
    let _ = git(&["config", scope.flag(), "--remove-section", &format!("merge.{DRIVER}")]);
    let _ = git(&["config", scope.flag(), "--remove-section", &format!("mergetool.{DRIVER}")]);
    println!("removed the '{DRIVER}' merge driver and mergetool from {}", scope.describe());

    // Only a file that is actually configured is touched. Looking one up the
    // way `install` does would register a default file on the way out.
    let Some(attributes) = scope.configured_attributes_path()? else { return Ok(0) };
    let Some(kept) = remove_attributes(&attributes)? else { return Ok(0) };

    // A file that held nothing else goes away, when it is one `install` would
    // have created: a repository's `.gitattributes`, or the default file
    // `install --global` registers. A file at a path of the user's own stays,
    // however empty, along with the setting that names it.
    let default_file = matches!(scope, Scope::Global)
        && default_global_attributes().ok().as_ref() == Some(&attributes);
    let ours_only = kept.trim().is_empty();
    if ours_only && (default_file || matches!(scope, Scope::Repo(_))) {
        std::fs::remove_file(&attributes)
            .with_context(|| format!("removing {}", attributes.display()))?;
        println!("removed {}, which held nothing but gdmerge rules", attributes.display());
        if default_file {
            git(&["config", "--global", "--unset", "core.attributesfile"])?;
            println!("unset core.attributesfile, which named it");
        }
        return Ok(0);
    }

    std::fs::write(&attributes, &kept)
        .with_context(|| format!("writing {}", attributes.display()))?;
    println!("removed gdmerge rules from {}", attributes.display());
    if ours_only {
        println!(
            "left the file in place, empty, and core.attributesfile naming it: both are yours, \
             not gdmerge git-install's"
        );
    } else {
        println!("left the file in place: it has rules gdmerge did not write");
        if matches!(scope, Scope::Global) {
            println!("core.attributesfile still names it");
        }
    }
    Ok(0)
}

enum Scope {
    Global,
    Repo(PathBuf),
}

impl Scope {
    fn detect(global: bool) -> Result<Scope> {
        if global {
            return Ok(Scope::Global);
        }
        let root = git(&["rev-parse", "--show-toplevel"])
            .context("not inside a git repository; use --global to configure your user instead")?;
        Ok(Scope::Repo(PathBuf::from(root.trim())))
    }

    fn flag(&self) -> &'static str {
        match self {
            Scope::Global => "--global",
            Scope::Repo(_) => "--local",
        }
    }

    fn describe(&self) -> String {
        match self {
            Scope::Global => "your global git config".to_string(),
            Scope::Repo(p) => format!("{}", p.join(".git/config").display()),
        }
    }

    /// The attributes file `install` writes to. For the user account this
    /// registers git's default location as `core.attributesfile` when nothing
    /// is configured yet.
    fn attributes_path(&self) -> Result<PathBuf> {
        match self {
            Scope::Repo(root) => Ok(root.join(".gitattributes")),
            Scope::Global => {
                if let Some(path) = configured_global_attributes() {
                    return Ok(path);
                }
                let path = default_global_attributes()?;
                git(&["config", "--global", "core.attributesfile", &path.to_string_lossy()])?;
                Ok(path)
            }
        }
    }

    /// The attributes file as configured right now, registering nothing.
    fn configured_attributes_path(&self) -> Result<Option<PathBuf>> {
        match self {
            Scope::Repo(root) => Ok(Some(root.join(".gitattributes"))),
            Scope::Global => Ok(configured_global_attributes()),
        }
    }
}

/// `core.attributesfile` from the global config, if set.
fn configured_global_attributes() -> Option<PathBuf> {
    let p = git(&["config", "--global", "core.attributesfile"]).ok()?;
    let p = p.trim();
    (!p.is_empty()).then(|| PathBuf::from(shellexpand_home(p)))
}

/// Where git looks for the user's attributes when `core.attributesfile` is
/// not set, which is the only file `install --global` ever registers.
fn default_global_attributes() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .context("cannot locate a config directory for the global attributes file")?;
    Ok(base.join("git").join("attributes"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

fn shellexpand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), home()) {
        (Some(rest), Some(h)) => h.join(rest).to_string_lossy().into_owned(),
        _ => p.to_string(),
    }
}

fn set(scope: &Scope, key: &str, value: &str) -> Result<()> {
    git(&["config", scope.flag(), key, value]).map(|_| ())
}

fn git(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .context("running git (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Appends the attribute lines, leaving any existing content untouched.
/// Returns false when they were already present.
fn add_attributes(path: &Path) -> Result<bool> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let wanted: Vec<String> = PATTERNS.iter().map(|p| format!("{p} merge={DRIVER}")).collect();
    if wanted.iter().all(|line| existing.lines().any(|l| l.trim() == line)) {
        return Ok(false);
    }
    let mut out = existing;
    let mut marker = MARKER.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
            marker.push_str(MARKER_NOTE);
        }
        out.push('\n');
    }
    out.push_str(&marker);
    out.push('\n');
    for line in wanted {
        out.push_str(&line);
        out.push('\n');
    }
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// What the file holds once the lines `add_attributes` wrote are taken out,
/// byte for byte, or `None` when there were none to take out.
///
/// Nothing is written here; the caller decides whether what is left is a file
/// worth keeping. Everything that is not gdmerge's is kept exactly as it was,
/// line endings and a missing final newline included: the blank line written
/// before the block goes, and so does the newline given to the previous last
/// line when the marker records that one was.
fn remove_attributes(path: &Path) -> Result<Option<String>> {
    let Ok(existing) = std::fs::read_to_string(path) else { return Ok(None) };
    let drop: Vec<String> = PATTERNS.iter().map(|p| format!("{p} merge={DRIVER}")).collect();
    let noted = format!("{MARKER}{MARKER_NOTE}");
    let mut kept = String::new();
    let mut removed = false;
    for piece in existing.split_inclusive('\n') {
        let line = piece.trim_end_matches(['\n', '\r']).trim();
        if line == MARKER || line == noted {
            removed = true;
            if kept.ends_with("\n\n") {
                kept.pop();
            }
            if line == noted && kept.ends_with('\n') {
                kept.pop();
            }
        } else if drop.iter().any(|d| d == line) {
            removed = true;
        } else {
            kept.push_str(piece);
        }
    }
    Ok(removed.then_some(kept))
}
