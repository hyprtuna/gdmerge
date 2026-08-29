//! `gdmerge`: semantic diff and 3-way merge for Godot 4 `.tscn` / `.tres` files.

mod check;
mod diff;
mod fallback;
mod gitcfg;
mod io;
mod merge;
mod mergetool;
mod report;
mod show;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

/// Exit status used for "the operation completed, but the result has conflicts".
pub const EXIT_CONFLICT: i32 = 1;

#[derive(Parser)]
#[command(
    name = "gdmerge",
    version,
    about = "Semantic diff and 3-way merge for Godot 4 scenes and resources (.tscn/.tres)",
    long_about = "Semantic diff and 3-way merge for Godot 4 scenes and resources (.tscn/.tres).\n\n\
                  Run `gdmerge git-install` once to register it as a git merge driver; git then \
                  resolves scene merges that would otherwise conflict."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what changed between two scene or resource files.
    Diff(DiffArgs),
    /// Three-way merge. Also accepts git's merge-driver argument order.
    Merge(MergeArgs),
    /// Redo a conflicted merge and show what the two branches disagreed about.
    ///
    /// This is what `git mergetool --tool=gdmerge` runs.
    Mergetool(MergetoolArgs),
    /// Parse, round-trip and structurally validate files.
    Check(CheckArgs),
    /// Register gdmerge as a git merge driver for *.tscn and *.tres.
    GitInstall(GitArgs),
    /// Undo `git-install`.
    GitUninstall(GitArgs),
}

#[derive(Args)]
struct DiffArgs {
    /// The "before" file.
    before: std::path::PathBuf,
    /// The "after" file.
    after: std::path::PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct MergeArgs {
    /// Common ancestor.
    #[arg(short = 'b', long = "base", value_name = "FILE")]
    base: Option<std::path::PathBuf>,
    /// Our version.
    #[arg(short = 'o', long = "ours", value_name = "FILE")]
    ours: Option<std::path::PathBuf>,
    /// Their version.
    #[arg(short = 't', long = "theirs", value_name = "FILE")]
    theirs: Option<std::path::PathBuf>,
    /// Write the result here instead of standard output.
    #[arg(short = 'O', long = "output", value_name = "FILE")]
    output: Option<std::path::PathBuf>,
    /// Length of the conflict marker runs (git's %L).
    #[arg(long, default_value_t = 7)]
    marker_size: usize,
    /// Label for our side in conflict markers.
    #[arg(long, default_value = "ours")]
    ours_label: String,
    /// Label for their side in conflict markers.
    #[arg(long, default_value = "theirs")]
    theirs_label: String,
    /// Positional form used by git: %O %A %B [%L] [%P].
    #[arg(value_name = "ARGS")]
    positional: Vec<String>,
}

#[derive(Args)]
struct MergetoolArgs {
    /// Common ancestor, git's $BASE.
    base: std::path::PathBuf,
    /// Our version, git's $LOCAL.
    ours: std::path::PathBuf,
    /// Their version, git's $REMOTE.
    theirs: std::path::PathBuf,
    /// File to write the result to, git's $MERGED. Defaults to the ours file.
    merged: Option<std::path::PathBuf>,
    /// Length of the conflict marker runs.
    #[arg(long, default_value_t = 7)]
    marker_size: usize,
    /// Label for our side in conflict markers.
    #[arg(long, default_value = "ours")]
    ours_label: String,
    /// Label for their side in conflict markers.
    #[arg(long, default_value = "theirs")]
    theirs_label: String,
}

#[derive(Args)]
struct CheckArgs {
    /// Files to validate. None is fine: a repository with no scene files
    /// yet has nothing wrong with it.
    files: Vec<std::path::PathBuf>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct GitArgs {
    /// Configure the current user instead of the current repository.
    #[arg(long)]
    global: bool,
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("gdmerge: {err:#}");
            std::process::exit(2);
        }
    }
}

fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Diff(a) => diff::run(&a.before, &a.after, a.json),
        Command::Merge(a) => merge::run(a),
        Command::Mergetool(a) => {
            let opts = tscn::MergeOptions {
                ours_label: a.ours_label.clone(),
                theirs_label: a.theirs_label.clone(),
                marker_size: a.marker_size,
            };
            mergetool::run(&a.base, &a.ours, &a.theirs, a.merged.as_ref(), &opts)
        }
        Command::Check(a) => check::run(&a.files, a.json),
        Command::GitInstall(a) => gitcfg::install(a.global),
        Command::GitUninstall(a) => gitcfg::uninstall(a.global),
    }
}
