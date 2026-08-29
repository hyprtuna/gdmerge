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
    match remove_attributes(&attributes)? {
        Removal::Nothing => {}
        Removal::Kept => {
            println!("removed gdmerge rules from {}", attributes.display());
            println!("left the file in place: it has rules gdmerge did not write");
            if matches!(scope, Scope::Global) {
                println!("core.attributesfile still names it");
            }
        }
        Removal::Emptied => {
            println!("removed {}, which held nothing but gdmerge rules", attributes.display());
            if matches!(scope, Scope::Global) {
                if Some(&attributes) == default_global_attributes().ok().as_ref() {
                    git(&["config", "--global", "--unset", "core.attributesfile"])?;
                    println!("unset core.attributesfile, which named it");
                } else {
                    println!(
                        "left core.attributesfile set: it names a file gdmerge git-install did not \
                         register"
                    );
                }
            }
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
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(MARKER);
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

/// What `remove_attributes` did to the file.
enum Removal {
    /// The file is missing or has no gdmerge lines in it.
    Nothing,
    /// gdmerge's lines are gone and the rest of the file is as it was.
    Kept,
    /// Nothing but gdmerge's lines was in the file, so it is gone too.
    Emptied,
}

/// Removes the lines `add_attributes` wrote and nothing else. A file that held
/// nothing else is deleted rather than left empty, which is what putting it
/// back the way it was means when `add_attributes` created it.
fn remove_attributes(path: &Path) -> Result<Removal> {
    let Ok(existing) = std::fs::read_to_string(path) else { return Ok(Removal::Nothing) };
    let drop: Vec<String> = PATTERNS.iter().map(|p| format!("{p} merge={DRIVER}")).collect();
    let kept: Vec<&str> = existing
        .lines()
        .filter(|l| l.trim() != MARKER && !drop.iter().any(|d| d == l.trim()))
        .collect();
    if kept.len() == existing.lines().count() {
        return Ok(Removal::Nothing);
    }
    let out = kept.join("\n");
    let out = out.trim_end_matches('\n');
    if out.trim().is_empty() {
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        return Ok(Removal::Emptied);
    }
    std::fs::write(path, format!("{out}\n"))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(Removal::Kept)
}
