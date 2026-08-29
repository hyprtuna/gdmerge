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
const COMMAND: &str = "gdmerge merge %O %A %B %L %P";
const MARKER: &str = "# gdmerge";
const PATTERNS: [&str; 2] = ["*.tscn", "*.tres"];

pub fn install(global: bool) -> Result<i32> {
    let scope = Scope::detect(global)?;
    set(&scope, &format!("merge.{DRIVER}.name"), NAME)?;
    set(&scope, &format!("merge.{DRIVER}.driver"), COMMAND)?;
    // Tells git how to merge the virtual ancestor of a criss-cross merge: keep
    // one side rather than attempting a text merge of two scene files.
    set(&scope, &format!("merge.{DRIVER}.recursive"), "binary")?;

    let attributes = scope.attributes_path()?;
    let added = add_attributes(&attributes)?;

    println!("configured the '{DRIVER}' merge driver in {}", scope.describe());
    if added {
        println!("added *.tscn and *.tres rules to {}", attributes.display());
    } else {
        println!("{} already had gdmerge rules", attributes.display());
    }
    if !global {
        println!("\ncommit .gitattributes so the whole team gets the same behaviour;");
        println!("each teammate still runs `gdmerge git-install` once to define the driver.");
    }
    Ok(0)
}

pub fn uninstall(global: bool) -> Result<i32> {
    let scope = Scope::detect(global)?;
    let _ = git(&["config", scope.flag(), "--remove-section", &format!("merge.{DRIVER}")]);
    let attributes = scope.attributes_path()?;
    let removed = remove_attributes(&attributes)?;
    println!("removed the '{DRIVER}' merge driver from {}", scope.describe());
    if removed {
        println!("removed gdmerge rules from {}", attributes.display());
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

    fn attributes_path(&self) -> Result<PathBuf> {
        match self {
            Scope::Repo(root) => Ok(root.join(".gitattributes")),
            Scope::Global => {
                if let Ok(p) = git(&["config", "--global", "core.attributesfile"]) {
                    let p = p.trim();
                    if !p.is_empty() {
                        return Ok(PathBuf::from(shellexpand_home(p)));
                    }
                }
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| home().map(|h| h.join(".config")))
                    .context("cannot locate a config directory for the global attributes file")?;
                let path = base.join("git").join("attributes");
                git(&["config", "--global", "core.attributesfile", &path.to_string_lossy()])?;
                Ok(path)
            }
        }
    }
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

/// Removes the lines `add_attributes` wrote and nothing else.
fn remove_attributes(path: &Path) -> Result<bool> {
    let Ok(existing) = std::fs::read_to_string(path) else { return Ok(false) };
    let drop: Vec<String> = PATTERNS.iter().map(|p| format!("{p} merge={DRIVER}")).collect();
    let kept: Vec<&str> = existing
        .lines()
        .filter(|l| l.trim() != MARKER && !drop.iter().any(|d| d == l.trim()))
        .collect();
    if kept.len() == existing.lines().count() {
        return Ok(false);
    }
    let mut out = kept.join("\n");
    let out_trimmed = out.trim_end_matches('\n');
    out = if out_trimmed.is_empty() { String::new() } else { format!("{out_trimmed}\n") };
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}
