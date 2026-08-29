//! End-to-end tests that run the real binary, including a real `git merge`
//! driven through the installed merge driver.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_gdmerge");

const BASE: &str = "\
[gd_scene load_steps=2 format=3 uid=\"uid://cli_demo\"]

[ext_resource type=\"Texture2D\" uid=\"uid://tex_ground\" path=\"res://ground.png\" id=\"1_ground\"]

[node name=\"Level\" type=\"Node2D\"]

[node name=\"Ground\" type=\"Sprite2D\" parent=\".\"]
texture = ExtResource(\"1_ground\")
";

// Both branches add a resource in the same place, and Godot happened to mint the
// same id for each. A text merge sees two different lines inserted at one point.
const OURS: &str = "\
[gd_scene load_steps=3 format=3 uid=\"uid://cli_demo\"]

[ext_resource type=\"Texture2D\" uid=\"uid://tex_ground\" path=\"res://ground.png\" id=\"1_ground\"]
[ext_resource type=\"Texture2D\" uid=\"uid://tex_player\" path=\"res://player.png\" id=\"2_added\"]

[node name=\"Level\" type=\"Node2D\"]

[node name=\"Ground\" type=\"Sprite2D\" parent=\".\"]
texture = ExtResource(\"1_ground\")

[node name=\"Player\" type=\"Sprite2D\" parent=\".\"]
texture = ExtResource(\"2_added\")
";

const THEIRS: &str = "\
[gd_scene load_steps=3 format=3 uid=\"uid://cli_demo\"]

[ext_resource type=\"Texture2D\" uid=\"uid://tex_ground\" path=\"res://ground.png\" id=\"1_ground\"]
[ext_resource type=\"AudioStream\" uid=\"uid://snd_step\" path=\"res://step.ogg\" id=\"2_added\"]

[node name=\"Level\" type=\"Node2D\"]

[node name=\"Ground\" type=\"Sprite2D\" parent=\".\"]
texture = ExtResource(\"1_ground\")

[node name=\"Steps\" type=\"AudioStreamPlayer\" parent=\".\"]
stream = ExtResource(\"2_added\")
";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("gdmerge-test-{name}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("creating the scratch directory");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, contents).expect("writing a scratch file");
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gdmerge(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("running gdmerge")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn check_accepts_a_valid_file() {
    let s = Scratch::new("check-ok");
    let f = s.write("ok.tscn", BASE);
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("1 file checked, 0 failed"));
}

#[test]
fn check_rejects_a_dangling_reference() {
    let s = Scratch::new("check-dangling");
    let broken = BASE.replace("ExtResource(\"1_ground\")\n", "ExtResource(\"9_missing\")\n");
    let f = s.write("broken.tscn", &broken);
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("dangling ExtResource(\"9_missing\")"), "{}", stdout(&out));
}

/// The README and the pre-commit hook say a colliding sibling index stops a
/// commit. Godot writes the index quoted, so that is the form which has to fail.
#[test]
fn check_fails_on_colliding_quoted_sibling_indices() {
    let s = Scratch::new("check-index");
    let f = s.write(
        "index.tscn",
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"A\" type=\"Node\" parent=\".\" index=\"0\"]\n\n\
         [node name=\"B\" type=\"Node\" parent=\".\" index=\"0\"]\n",
    );
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).contains("2 children of \".\" share index 0"), "{}", stdout(&out));
}

#[test]
fn check_reports_a_parse_error_with_a_line_number() {
    let s = Scratch::new("check-parse");
    let f = s.write("bad.tscn", "[gd_scene format=3]\n\n[node name=\"R\"]\nx = Vector2(1\n");
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("parse error: line"), "{}", stdout(&out));
}

#[test]
fn diff_reports_semantic_changes() {
    let s = Scratch::new("diff");
    let a = s.write("a.tscn", BASE);
    let b = s.write("b.tscn", OURS);
    let out = gdmerge(&["diff", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("+ node Player"), "{text}");
    assert!(text.contains("+ ext_resource uid://tex_player"), "{text}");
}

#[test]
fn diff_emits_json() {
    let s = Scratch::new("diff-json");
    let a = s.write("a.tscn", BASE);
    let b = s.write("b.tscn", OURS);
    let out = gdmerge(&["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let changes = parsed["changes"].as_array().expect("a changes array");
    assert!(changes.iter().any(|c| c["kind"] == "added"));
}

#[test]
fn diff_of_identical_files_is_empty() {
    let s = Scratch::new("diff-same");
    let a = s.write("a.tscn", BASE);
    let b = s.write("b.tscn", BASE);
    let out = gdmerge(&["diff", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("no semantic changes"));
}

#[test]
fn merge_resolves_colliding_resource_ids() {
    let s = Scratch::new("merge-ids");
    let b = s.write("base.tscn", BASE);
    let o = s.write("ours.tscn", OURS);
    let t = s.write("theirs.tscn", THEIRS);
    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout(&out);
    assert!(text.contains("uid://tex_player"), "{text}");
    assert!(text.contains("uid://snd_step"), "{text}");
    assert!(text.contains("name=\"Player\""), "{text}");
    assert!(text.contains("name=\"Steps\""), "{text}");
    assert!(!text.contains("<<<<<<<"), "{text}");
    // The two resources arrived with the same id; exactly one keeps it.
    assert_eq!(text.matches("id=\"2_added\"").count(), 1, "{text}");
}

#[test]
fn merge_conflicts_are_localised_and_exit_one() {
    let s = Scratch::new("merge-conflict");
    let b = s.write("base.tscn", BASE);
    let o = s.write(
        "ours.tscn",
        &BASE.replace(
            "[node name=\"Level\" type=\"Node2D\"]",
            "[node name=\"Level\" type=\"Node2D\"]\nrotation = 1.0",
        ),
    );
    let t = s.write(
        "theirs.tscn",
        &BASE.replace(
            "[node name=\"Level\" type=\"Node2D\"]",
            "[node name=\"Level\" type=\"Node2D\"]\nrotation = 2.0",
        ),
    );
    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("<<<<<<< ours"), "{text}");
    assert!(text.contains(">>>>>>> theirs"), "{text}");
    // Only the conflicting node is wrapped; the untouched node stays clean.
    assert!(text.contains("[node name=\"Ground\" type=\"Sprite2D\" parent=\".\"]"), "{text}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("conflict in root node \"Level\""), "{stderr}");
}

#[test]
fn merge_falls_back_to_a_text_merge_on_unparseable_input() {
    let s = Scratch::new("merge-fallback");
    let b = s.write("base.tscn", "not a scene file\n");
    let o = s.write("ours.tscn", "not a scene file\nours\n");
    let t = s.write("theirs.tscn", "not a scene file\ntheirs\n");
    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("falling back to a text merge"), "{stderr}");
    assert_eq!(out.status.code(), Some(1), "a real text conflict must be reported");
    assert!(stdout(&out).contains("<<<<<<<"), "{}", stdout(&out));
}

#[test]
fn positional_form_writes_the_result_into_the_ours_file() {
    let s = Scratch::new("merge-positional");
    let b = s.write("base.tscn", BASE);
    let o = s.write("ours.tscn", OURS);
    let t = s.write("theirs.tscn", THEIRS);
    let out = gdmerge(&[
        "merge",
        b.to_str().unwrap(),
        o.to_str().unwrap(),
        t.to_str().unwrap(),
        "7",
        "level.tscn",
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).is_empty(), "the driver form must not print the result");
    let written = std::fs::read_to_string(&o).expect("ours was rewritten in place");
    assert!(written.contains("uid://snd_step"), "{written}");
    assert!(written.contains("name=\"Player\""), "{written}");
}

/// Base, ours and theirs that set one property three different ways, which no
/// merge algorithm can resolve on its own.
const CONFLICT_BASE: &str = "\
[gd_scene format=3 uid=\"uid://conflict\"]

[node name=\"Player\" type=\"CharacterBody2D\"]
speed = 100.0
";

fn conflict_side(speed: &str) -> String {
    CONFLICT_BASE.replace("speed = 100.0", &format!("speed = {speed}"))
}

#[test]
fn mergetool_prints_a_property_table_for_a_conflict() {
    let s = Scratch::new("mergetool-table");
    let b = s.write("base.tscn", CONFLICT_BASE);
    let o = s.write("ours.tscn", &conflict_side("250.0"));
    let t = s.write("theirs.tscn", &conflict_side("400.0"));
    let m = s.write("merged.tscn", "");

    let out = gdmerge(&[
        "mergetool",
        b.to_str().unwrap(),
        o.to_str().unwrap(),
        t.to_str().unwrap(),
        m.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("1 conflict in"), "{text}");
    assert!(text.contains("Conflict 1 of 1: root node \"Player\""), "{text}");
    assert!(text.contains("property"), "{text}");
    assert!(text.contains("base"), "{text}");
    // The differing row is marked, and the agreeing ones are not.
    let speed = text.lines().find(|l| l.contains("250.0")).expect("a speed row");
    assert!(speed.trim_start().starts_with('>'), "{speed}");
    assert!(speed.contains("400.0"), "both sides belong on one row: {speed}");
    let ty = text.lines().find(|l| l.contains("CharacterBody2D")).expect("a type row");
    assert!(!ty.trim_start().starts_with('>'), "{ty}");
    // And it wrote the merged file, markers and all.
    let written = std::fs::read_to_string(&m).expect("merged file written");
    assert!(written.contains("<<<<<<<"), "{written}");
}

#[test]
fn mergetool_shows_absence_for_a_delete_against_a_modify() {
    let s = Scratch::new("mergetool-delete");
    let b = s.write("base.tscn", CONFLICT_BASE);
    let o = s.write("ours.tscn", "[gd_scene format=3 uid=\"uid://conflict\"]\n");
    let t = s.write("theirs.tscn", &conflict_side("400.0"));
    let m = s.write("merged.tscn", "");

    let out = gdmerge(&[
        "mergetool",
        b.to_str().unwrap(),
        o.to_str().unwrap(),
        t.to_str().unwrap(),
        m.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("deleted by ours, modified by theirs"), "{text}");
    assert!(text.contains("(absent)"), "{text}");
}

#[test]
fn mergetool_reports_a_clean_merge_and_writes_it() {
    let s = Scratch::new("mergetool-clean");
    let b = s.write("base.tscn", BASE);
    let o = s.write("ours.tscn", OURS);
    let t = s.write("theirs.tscn", THEIRS);
    let m = s.write("merged.tscn", "");

    let out = gdmerge(&[
        "mergetool",
        b.to_str().unwrap(),
        o.to_str().unwrap(),
        t.to_str().unwrap(),
        m.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("no conflicts"), "{}", stdout(&out));
    let written = std::fs::read_to_string(&m).expect("merged file written");
    assert!(written.contains("uid://snd_step"), "{written}");
    assert!(!written.contains("<<<<<<<"), "{written}");
}

#[test]
fn the_merge_driver_explains_conflicts_on_stderr() {
    let s = Scratch::new("driver-explains");
    let b = s.write("base.tscn", CONFLICT_BASE);
    let o = s.write("ours.tscn", &conflict_side("250.0"));
    let t = s.write("theirs.tscn", &conflict_side("400.0"));

    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("speed: ours 250.0 / theirs 400.0"), "{err}");
    assert!(err.contains("git mergetool --tool=gdmerge"), "{err}");
    // Only the differing item is listed, not the whole node.
    assert!(!err.contains("CharacterBody2D"), "{err}");
}

/// A sub-resource used from more than one node is matched by its contents, so
/// editing it on both branches leaves two copies, theirs under a new id, and a
/// conflict at each node that references it. The stderr line has to show the
/// ids the file shows: it used to print the same value for both sides, because
/// it was composed before the renumbering.
#[test]
fn the_driver_reports_the_renumbered_id_on_a_shared_sub_resource_conflict() {
    let s = Scratch::new("driver-renumbered");
    let base = "[gd_scene load_steps=2 format=3 uid=\"uid://shared\"]\n\n\
                [sub_resource type=\"RectangleShape2D\" id=\"1_s\"]\n\
                size = Vector2(8, 8)\n\n\
                [node name=\"Root\" type=\"Node2D\"]\n\n\
                [node name=\"A\" type=\"CollisionShape2D\" parent=\".\"]\n\
                shape = SubResource(\"1_s\")\n\n\
                [node name=\"B\" type=\"CollisionShape2D\" parent=\".\"]\n\
                shape = SubResource(\"1_s\")\n";
    let b = s.write("base.tscn", base);
    let o = s.write("ours.tscn", &base.replace("Vector2(8, 8)", "Vector2(10, 10)"));
    let t = s.write("theirs.tscn", &base.replace("Vector2(8, 8)", "Vector2(24, 24)"));

    let out = merge_to_stdout(&b, &o, &t);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = err.lines().filter(|l| l.contains("shape: ours")).collect();
    assert_eq!(lines.len(), 2, "one line per conflicting node: {err}");
    for line in &lines {
        let (ours, theirs) = line.split_once(" / theirs ").expect("both sides on the line");
        let ours = ours.rsplit("ours ").next().unwrap_or_default();
        assert_ne!(ours, theirs, "the two sides shown must differ: {line}");
    }
    // And what is shown is what the markers hold, cut to the width the line
    // allows a value.
    let text = stdout(&out);
    assert!(text.contains("id=\"RectangleShape2D_gdm0\""), "{text}");
    assert!(
        lines[0].contains("ours SubResource(\"1_s\") / theirs SubResource(\"RectangleShape2D_g"),
        "{}",
        lines[0]
    );
}

// ---------------------------------------------------------------------------
// The real thing: a git merge that only succeeds because the driver is active.
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> Output {
    let bin_dir = Path::new(BIN).parent().expect("the test binary has a directory");
    let path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut parts = vec![bin_dir.to_path_buf()];
            parts.extend(std::env::split_paths(&existing));
            std::env::join_paths(parts).expect("building PATH")
        }
        None => bin_dir.as_os_str().to_owned(),
    };
    git_with_path(dir, args, path)
}

/// A `PATH` with every directory holding a `gdmerge` removed, so the driver's
/// own `command -v` check fails. Filtered rather than emptied, because git and
/// the shell it runs the driver through still have to be reachable.
fn path_without_gdmerge() -> std::ffi::OsString {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let kept: Vec<PathBuf> = std::env::split_paths(&existing)
        .filter(|dir| !dir.join("gdmerge").exists() && !dir.join("gdmerge.exe").exists())
        .collect();
    std::env::join_paths(kept).expect("building PATH")
}

fn git_with_path(dir: &Path, args: &[&str], path: std::ffi::OsString) -> Output {
    Command::new("git")
        .current_dir(dir)
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .args(args)
        .output()
        .expect("running git")
}

fn setup_repo(dir: &Path) {
    assert!(git(dir, &["init", "-q", "-b", "main"]).status.success());
    assert!(git(dir, &["config", "user.name", "Test"]).status.success());
    assert!(git(dir, &["config", "user.email", "test@example.invalid"]).status.success());
    std::fs::write(dir.join("level.tscn"), BASE).unwrap();
    assert!(git(dir, &["add", "."]).status.success());
    assert!(git(dir, &["commit", "-qm", "base"]).status.success());

    assert!(git(dir, &["checkout", "-qb", "feature"]).status.success());
    std::fs::write(dir.join("level.tscn"), THEIRS).unwrap();
    assert!(git(dir, &["commit", "-qam", "add footstep audio"]).status.success());

    assert!(git(dir, &["checkout", "-q", "main"]).status.success());
    std::fs::write(dir.join("level.tscn"), OURS).unwrap();
    assert!(git(dir, &["commit", "-qam", "add the player sprite"]).status.success());
}

#[test]
fn git_merge_conflicts_without_the_driver() {
    let s = Scratch::new("git-plain");
    setup_repo(s.path());
    let out = git(s.path(), &["merge", "--no-edit", "feature"]);
    assert!(!out.status.success(), "git should not resolve this on its own");
    let merged = std::fs::read_to_string(s.path().join("level.tscn")).unwrap();
    assert!(merged.contains("<<<<<<<"), "expected git's own conflict markers:\n{merged}");
}

#[test]
fn git_merge_succeeds_through_the_installed_driver() {
    let s = Scratch::new("git-driver");
    setup_repo(s.path());

    let install = Command::new(BIN)
        .current_dir(s.path())
        .arg("git-install")
        .output()
        .expect("running gdmerge git-install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));
    let attributes = std::fs::read_to_string(s.path().join(".gitattributes")).unwrap();
    assert!(attributes.contains("*.tscn merge=gdmerge"), "{attributes}");
    assert!(attributes.contains("*.tres merge=gdmerge"), "{attributes}");

    let out = git(s.path(), &["merge", "--no-edit", "feature"]);
    assert!(
        out.status.success(),
        "merge failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let merged = std::fs::read_to_string(s.path().join("level.tscn")).unwrap();
    assert!(!merged.contains("<<<<<<<"), "{merged}");
    assert!(merged.contains("uid://tex_player"), "{merged}");
    assert!(merged.contains("uid://snd_step"), "{merged}");
    assert!(merged.contains("name=\"Player\""), "{merged}");
    assert!(merged.contains("name=\"Steps\""), "{merged}");
    assert_eq!(merged.matches("id=\"2_added\"").count(), 1, "{merged}");

    // And the result is a file gdmerge itself considers well formed.
    let check = Command::new(BIN)
        .current_dir(s.path())
        .args(["check", "level.tscn"])
        .output()
        .expect("running gdmerge check");
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stdout));

    // git-uninstall puts everything back.
    let out = Command::new(BIN)
        .current_dir(s.path())
        .arg("git-uninstall")
        .output()
        .expect("running gdmerge git-uninstall");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let attributes = std::fs::read_to_string(s.path().join(".gitattributes")).unwrap_or_default();
    assert!(!attributes.contains("merge=gdmerge"), "{attributes}");
    let driver = git(s.path(), &["config", "--get", "merge.gdmerge.driver"]);
    assert!(!driver.status.success(), "the driver config should be gone");
    let tool = git(s.path(), &["config", "--get", "mergetool.gdmerge.cmd"]);
    assert!(!tool.status.success(), "the mergetool config should be gone");
}

/// Runs gdmerge against a scratch user account: its own `HOME`, so the global
/// git config and the default attributes file land in the scratch directory.
fn gdmerge_as_user(home: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_GLOBAL")
        .output()
        .expect("running gdmerge")
}

/// A global git config value as that scratch user sees it.
fn global_config(home: &Path, key: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--global", "--get", key])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_GLOBAL")
        .output()
        .expect("running git");
    out.status.success().then(|| stdout(&out).trim().to_string())
}

/// `git-install --global` registers a default attributes file and creates it;
/// `git-uninstall --global` has to take both away again, or the account is not
/// back the way it was.
#[test]
fn git_uninstall_global_removes_what_git_install_global_created() {
    let s = Scratch::new("global-uninstall");
    let home = s.path();
    let attributes = home.join("xdg").join("git").join("attributes");

    let out = gdmerge_as_user(home, &["git-install", "--global"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(global_config(home, "merge.gdmerge.driver").is_some());
    let registered = global_config(home, "core.attributesfile").expect("core.attributesfile set");
    assert_eq!(Path::new(&registered), attributes, "{registered}");
    let written = std::fs::read_to_string(&attributes).expect("attributes file created");
    assert!(written.contains("*.tscn merge=gdmerge"), "{written}");

    let out = gdmerge_as_user(home, &["git-uninstall", "--global"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(global_config(home, "merge.gdmerge.driver").is_none());
    assert!(global_config(home, "mergetool.gdmerge.cmd").is_none());
    assert!(global_config(home, "core.attributesfile").is_none(), "core.attributesfile left set");
    assert!(!attributes.exists(), "the emptied attributes file was left behind");
    assert!(stdout(&out).contains("unset core.attributesfile"), "{}", stdout(&out));
}

/// Rules somebody else added to the attributes file are not gdmerge's to
/// remove, so the file and the setting naming it stay, and the output says so.
#[test]
fn git_uninstall_global_keeps_an_attributes_file_with_other_rules() {
    let s = Scratch::new("global-uninstall-shared");
    let home = s.path();
    let attributes = home.join("xdg").join("git").join("attributes");

    let out = gdmerge_as_user(home, &["git-install", "--global"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut written = std::fs::read_to_string(&attributes).expect("attributes file created");
    written.push_str("*.png binary\n");
    std::fs::write(&attributes, written).unwrap();

    let out = gdmerge_as_user(home, &["git-uninstall", "--global"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(global_config(home, "merge.gdmerge.driver").is_none());
    let remaining = std::fs::read_to_string(&attributes).expect("the file has to stay");
    assert_eq!(remaining, "*.png binary\n");
    assert!(global_config(home, "core.attributesfile").is_some(), "the setting has to stay");
    assert!(stdout(&out).contains("left the file in place"), "{}", stdout(&out));
    assert!(stdout(&out).contains("core.attributesfile still names it"), "{}", stdout(&out));
}

/// A conflict the driver cannot resolve, handed to `git mergetool --tool=gdmerge`.
#[test]
fn git_mergetool_explains_a_real_conflict() {
    let s = Scratch::new("git-mergetool");
    let dir = s.path();
    assert!(git(dir, &["init", "-q", "-b", "main"]).status.success());
    assert!(git(dir, &["config", "user.name", "Test"]).status.success());
    assert!(git(dir, &["config", "user.email", "test@example.invalid"]).status.success());

    std::fs::write(dir.join("level.tscn"), CONFLICT_BASE).unwrap();
    assert!(git(dir, &["add", "."]).status.success());
    assert!(git(dir, &["commit", "-qm", "base"]).status.success());

    assert!(git(dir, &["checkout", "-qb", "faster"]).status.success());
    std::fs::write(dir.join("level.tscn"), conflict_side("400.0")).unwrap();
    assert!(git(dir, &["commit", "-qam", "speed up"]).status.success());

    assert!(git(dir, &["checkout", "-q", "main"]).status.success());
    std::fs::write(dir.join("level.tscn"), conflict_side("250.0")).unwrap();
    assert!(git(dir, &["commit", "-qam", "tune speed"]).status.success());

    let install = Command::new(BIN)
        .current_dir(dir)
        .arg("git-install")
        .output()
        .expect("running gdmerge git-install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));

    // The driver runs, cannot resolve it, and says why on stderr.
    let out = git(dir, &["merge", "--no-edit", "faster"]);
    assert!(!out.status.success(), "this conflict is not resolvable");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("speed: ours 250.0 / theirs 400.0"), "{err}");

    // Then the mergetool lays the two sides out.
    let tool = git(dir, &["mergetool", "--no-prompt", "--tool=gdmerge"]);
    let text = String::from_utf8_lossy(&tool.stdout);
    assert!(text.contains("Conflict 1 of 1"), "{text}");
    assert!(text.contains("speed"), "{text}");
    assert!(text.contains("250.0"), "{text}");
    assert!(text.contains("400.0"), "{text}");
}

// ---------------------------------------------------------------------------
// Godot 3 input. gdmerge does not understand it, so every one of these has to
// come out exactly as `git merge-file` would have left it.
// ---------------------------------------------------------------------------

/// The giveaway is the unquoted resource id, not `format=2`: Godot 3 wrote
/// `id=1` and `SubResource( 1 )`, neither of which gdmerge can renumber.
const LEGACY_BASE: &str = "\
[gd_scene load_steps=2 format=2]

[sub_resource type=\"RectangleShape2D\" id=1]
extents = Vector2( 8, 8 )

[node name=\"Root\" type=\"Node2D\"]

[node name=\"Body\" type=\"CollisionShape2D\" parent=\".\"]
position = Vector2( 0, 0 )
shape = SubResource( 1 )
";

fn legacy_extents(v: &str) -> String {
    LEGACY_BASE.replace("Vector2( 8, 8 )", v)
}

fn legacy_position(v: &str) -> String {
    LEGACY_BASE.replace("position = Vector2( 0, 0 )", &format!("position = {v}"))
}

/// Runs the text merge gdmerge is supposed to be indistinguishable from, with
/// the same flags and labels its fallback uses.
fn git_merge_file(ours: &Path, base: &Path, theirs: &Path) -> (String, Option<i32>) {
    let out = Command::new("git")
        .args(["merge-file", "-p", "--marker-size=7"])
        .args(["-L", "ours", "-L", "base", "-L", "theirs"])
        .args([ours, base, theirs])
        .output()
        .expect("running git merge-file");
    (stdout(&out), out.status.code())
}

fn merge_to_stdout(base: &Path, ours: &Path, theirs: &Path) -> Output {
    gdmerge(&[
        "merge",
        "--base",
        base.to_str().unwrap(),
        "--ours",
        ours.to_str().unwrap(),
        "--theirs",
        theirs.to_str().unwrap(),
    ])
}

/// Every error `gdmerge check` reports for one file.
fn check_errors(path: &Path) -> Vec<String> {
    let out = gdmerge(&["check", "--json", path.to_str().unwrap()]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    parsed[0]["issues"]
        .as_array()
        .map(|issues| {
            issues
                .iter()
                .filter(|i| i["severity"] == "error")
                .map(|i| i["message"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_legacy_clean_merge_is_byte_identical_to_git_merge_file() {
    let s = Scratch::new("legacy-clean");
    let b = s.write("base.tscn", LEGACY_BASE);
    let o = s.write("ours.tscn", &legacy_extents("Vector2( 10, 10 )"));
    let t = s.write("theirs.tscn", &legacy_position("Vector2( 5, 5 )"));

    let out = merge_to_stdout(&b, &o, &t);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("falling back to a text merge"), "{err}");
    assert!(err.contains("base.tscn"), "the message has to name the file: {err}");
    assert!(err.contains("Godot 3 unquoted id form"), "the message has to say why: {err}");

    let (expected, expected_code) = git_merge_file(&o, &b, &t);
    assert_eq!(out.status.code(), expected_code);
    assert_eq!(stdout(&out), expected, "the fallback must reproduce git's own merge exactly");
    // And what git produced is what the pre-0.3.2 semantic path silently broke:
    // one id renumbered, the reference to it left behind.
    assert!(stdout(&out).contains("shape = SubResource( 1 )"), "{}", stdout(&out));
    assert!(stdout(&out).contains("id=1]"), "{}", stdout(&out));
}

#[test]
fn a_legacy_conflict_is_byte_identical_to_git_merge_file() {
    let s = Scratch::new("legacy-conflict");
    let b = s.write("base.tscn", LEGACY_BASE);
    let o = s.write("ours.tscn", &legacy_extents("Vector2( 10, 10 )"));
    let t = s.write("theirs.tscn", &legacy_extents("Vector2( 24, 24 )"));

    let out = merge_to_stdout(&b, &o, &t);
    let (expected, expected_code) = git_merge_file(&o, &b, &t);
    assert_eq!(expected_code, Some(1), "the fixture is supposed to conflict in git's own terms");
    assert_eq!(out.status.code(), expected_code, "git's exit status has to be passed through");
    assert_eq!(stdout(&out), expected, "the conflict markers must be git's own, byte for byte");
    assert!(stdout(&out).contains("<<<<<<< ours"), "{}", stdout(&out));
}

/// The trigger is the id spelling, not the `format` number. A file that claims
/// `format=3` and still writes Godot 3 ids has to take the same path.
#[test]
fn unquoted_ids_in_a_format_3_file_take_the_same_path() {
    let s = Scratch::new("legacy-format3");
    let modern = |src: &str| src.replace("format=2", "format=3");
    let b = s.write("base.tscn", &modern(LEGACY_BASE));
    let o = s.write("ours.tscn", &modern(&legacy_extents("Vector2( 10, 10 )")));
    let t = s.write("theirs.tscn", &modern(&legacy_position("Vector2( 5, 5 )")));

    let out = merge_to_stdout(&b, &o, &t);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("falling back to a text merge"), "{err}");
    let (expected, expected_code) = git_merge_file(&o, &b, &t);
    assert_eq!(out.status.code(), expected_code);
    assert_eq!(stdout(&out), expected);
}

/// Runs `gdmerge merge` to stdout with one git config file standing in for the
/// user's, so a `merge.conflictstyle` setting can be tried both ways.
fn merge_with_gitconfig(base: &Path, ours: &Path, theirs: &Path, config: &Path) -> Output {
    Command::new(BIN)
        .args(["merge", "--base"])
        .arg(base)
        .arg("--ours")
        .arg(ours)
        .arg("--theirs")
        .arg(theirs)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("running gdmerge")
}

/// The text fallback is git's own merge, so its markers follow the user's
/// `merge.conflictstyle` rather than a style chosen here. gdmerge's own markers
/// are always the two-sided form; the mergetool is what shows the base value.
#[test]
fn the_text_fallback_follows_git_conflictstyle_and_the_semantic_merge_does_not() {
    let s = Scratch::new("conflictstyle");
    let diff3 = s.write("diff3.gitconfig", "[merge]\n\tconflictstyle = diff3\n");
    let plain = s.write("plain.gitconfig", "");

    // A legacy file goes to the fallback, and this pair conflicts there.
    let b = s.write("base.tscn", LEGACY_BASE);
    let o = s.write("ours.tscn", &legacy_extents("Vector2( 10, 10 )"));
    let t = s.write("theirs.tscn", &legacy_extents("Vector2( 24, 24 )"));

    let out = merge_with_gitconfig(&b, &o, &t, &diff3);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("||||||| base"), "{}", stdout(&out));

    let out = merge_with_gitconfig(&b, &o, &t, &plain);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("<<<<<<< ours"), "{}", stdout(&out));
    assert!(!stdout(&out).contains("|||||||"), "{}", stdout(&out));

    // A conflict gdmerge raises itself has no base section either way.
    let b = s.write("cbase.tscn", CONFLICT_BASE);
    let o = s.write("cours.tscn", &conflict_side("250.0"));
    let t = s.write("ctheirs.tscn", &conflict_side("400.0"));
    let out = merge_with_gitconfig(&b, &o, &t, &diff3);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("<<<<<<< ours"), "{}", stdout(&out));
    assert!(!stdout(&out).contains("|||||||"), "{}", stdout(&out));
}

/// `-O` may point at something that is not a file to be replaced, such as
/// `/dev/null` from a script that only wants the exit status. The result is
/// normally written beside the target and renamed over it, which a device
/// cannot be, so such a target is written directly.
#[cfg(unix)]
#[test]
fn merge_writes_directly_to_a_target_that_is_not_a_regular_file() {
    let s = Scratch::new("dev-null");
    let b = s.write("base.tscn", BASE);
    let o = s.write("ours.tscn", OURS);
    let t = s.write("theirs.tscn", THEIRS);
    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
        "-O",
        "/dev/null",
    ]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.is_empty(), "{err}");
}

/// `check` has to see the legacy reference spelling, or a dangling
/// `SubResource( 1 )` is invisible and the merge that created it looks clean.
#[test]
fn check_reports_a_dangling_unquoted_reference() {
    let s = Scratch::new("legacy-dangling");
    let broken = LEGACY_BASE.replace("id=1]", "id=\"RectangleShape2D_x\"]");
    let f = s.write("broken.tscn", &broken);
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("dangling SubResource(\"1\")"), "{}", stdout(&out));
}

/// The reference spelling is enough on its own: a quoted declaration referenced
/// as `SubResource( 1 )` cannot be renumbered either.
#[test]
fn check_names_the_legacy_reference_form() {
    let s = Scratch::new("legacy-ref");
    let f = s.write("legacy.tscn", &LEGACY_BASE.replace("id=1]", "id=\"1\"]"));
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("SubResource(1) uses the Godot 3 unquoted id form"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_names_the_legacy_id_form() {
    let s = Scratch::new("legacy-id");
    let f = s.write("legacy.tscn", LEGACY_BASE);
    let out = gdmerge(&["check", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("Godot 3 unquoted id form"), "{}", stdout(&out));
}

/// The invariant that catches this class of bug: whatever a merge writes must
/// not be broken in a way its inputs were not. A legacy file is already one
/// `check` rejects, so the output is allowed to carry the same complaints and
/// nothing else. The 0.3.1 output failed this: it introduced a dangling
/// `SubResource( 1 )` that neither input had.
#[test]
fn merging_legacy_input_introduces_no_new_breakage() {
    let s = Scratch::new("legacy-invariant");
    let b = s.write("base.tscn", LEGACY_BASE);
    let o = s.write("ours.tscn", &legacy_extents("Vector2( 10, 10 )"));
    let t = s.write("theirs.tscn", &legacy_position("Vector2( 5, 5 )"));
    let merged = s.path().join("out.tscn");

    let out = gdmerge(&[
        "merge",
        "--base",
        b.to_str().unwrap(),
        "--ours",
        o.to_str().unwrap(),
        "--theirs",
        t.to_str().unwrap(),
        "-O",
        merged.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));

    let mut inherited: Vec<String> = Vec::new();
    for input in [&b, &o, &t] {
        inherited.extend(check_errors(input));
    }
    for error in check_errors(&merged) {
        assert!(
            inherited.contains(&error),
            "the merge introduced an error none of its inputs had: {error}"
        );
    }
}

/// A reference that was already broken in the common ancestor is not the
/// merge's doing, and the library passes it through. The command does not get
/// that far: every input has to pass `check` first, a `NodePath` naming nothing
/// fails it, and so the whole merge goes to git's text merge with git's exit
/// status. The 0.3.0 notes claimed otherwise; this pins what ships.
#[test]
fn a_pre_existing_broken_reference_hands_the_merge_to_git() {
    let s = Scratch::new("inherited-breakage");
    let base = "[gd_scene format=3 uid=\"uid://inherited\"]\n\n\
                [node name=\"Level\" type=\"Node2D\"]\n\
                stale = NodePath(\"Ghost\")\n\n\
                [node name=\"Hero\" type=\"Node2D\" parent=\".\"]\n";
    let ours = base.replace(
        "[node name=\"Hero\"",
        "[node name=\"Extra\" type=\"Node\" parent=\".\"]\n\n[node name=\"Hero\"",
    );
    let theirs = format!("{base}\n[node name=\"Other\" type=\"Node\" parent=\".\"]\n");
    let b = s.write("base.tscn", base);
    let o = s.write("ours.tscn", &ours);
    let t = s.write("theirs.tscn", &theirs);

    let out = merge_to_stdout(&b, &o, &t);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("falling back to a text merge"), "{err}");
    assert!(err.contains("base.tscn"), "the message has to name the file: {err}");
    assert!(err.contains("NodePath(\"Ghost\")"), "the message has to say why: {err}");

    let (expected, expected_code) = git_merge_file(&o, &b, &t);
    assert_eq!(out.status.code(), expected_code, "git's exit status has to be passed through");
    assert_eq!(stdout(&out), expected, "the fallback must reproduce git's own merge exactly");
    // The two additions do not overlap, so git merges them cleanly on its own:
    // what is lost is the semantic merge, not anybody's work.
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("name=\"Extra\""), "{}", stdout(&out));
    assert!(stdout(&out).contains("name=\"Other\""), "{}", stdout(&out));
}

/// The driver is configured per repository and committed as `.gitattributes`,
/// so a teammate who has not installed gdmerge still has git calling it. If
/// that call simply fails, git leaves `%A` holding only our side, marks the
/// file conflicted, and puts no markers in it: staging it drops their work
/// without a word. The worst case has to be git's own text merge instead.
#[test]
fn the_driver_falls_back_to_git_when_gdmerge_is_not_installed() {
    let s = Scratch::new("driver-missing");
    setup_repo(s.path());

    let install = Command::new(BIN)
        .current_dir(s.path())
        .arg("git-install")
        .output()
        .expect("running gdmerge git-install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));

    let out = git_with_path(s.path(), &["merge", "--no-edit", "feature"], path_without_gdmerge());
    assert!(!out.status.success(), "a text merge of this pair does conflict");

    let merged = std::fs::read_to_string(s.path().join("level.tscn")).unwrap();
    assert!(merged.contains("<<<<<<< ours"), "no conflict markers at all:\n{merged}");
    assert!(merged.contains(">>>>>>> theirs"), "{merged}");
    // Both sides survived, which is the whole point: neither can be lost by a
    // `git add` that trusts what is in the file.
    assert!(merged.contains("uid://tex_player"), "our side is missing:\n{merged}");
    assert!(merged.contains("uid://snd_step"), "their side is missing:\n{merged}");
}

/// A merge that strands a `NodePath` has to reach the user the same way any
/// other conflict does: markers in the file and a table with the row to
/// resolve. Before 0.3.2 the file came back clean-looking and the table empty.
const STRAND_BASE: &str = "\
[gd_scene format=3 uid=\"uid://strand\"]

[node name=\"Level\" type=\"Node2D\"]

[node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]

[node name=\"Shadow\" type=\"RemoteTransform2D\" parent=\".\"]
update_rotation = true
";

#[test]
fn mergetool_shows_the_row_for_a_stranded_node_path() {
    let s = Scratch::new("mergetool-strand");
    let b = s.write("base.tscn", STRAND_BASE);
    let o = s.write(
        "ours.tscn",
        &STRAND_BASE.replace("[node name=\"Hero\" type=\"CharacterBody2D\" parent=\".\"]\n\n", ""),
    );
    let t = s.write("theirs.tscn", &format!("{STRAND_BASE}remote_path = NodePath(\"../Hero\")\n"));
    let m = s.write("merged.tscn", "");

    // Each input is sound on its own; only the merge strands the path.
    for input in [&b, &o, &t] {
        assert!(check_errors(input).is_empty(), "{}: {:?}", input.display(), check_errors(input));
    }

    let out = gdmerge(&[
        "mergetool",
        b.to_str().unwrap(),
        o.to_str().unwrap(),
        t.to_str().unwrap(),
        m.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("Conflict 1 of 1: remote_path on node \"Shadow\""), "{text}");
    let row = text
        .lines()
        .find(|l| l.trim_start().starts_with('>') && l.contains("remote_path"))
        .unwrap_or_else(|| panic!("no marked remote_path row in:\n{text}"));
    assert!(row.contains("NodePath(\"../Hero\")"), "{row}");

    // The file it wrote carries the markers, around that node only.
    let written = std::fs::read_to_string(&m).expect("merged file written");
    assert!(written.contains("<<<<<<< ours"), "{written}");
    assert!(written.contains(">>>>>>> theirs"), "{written}");
    assert!(written.contains("[node name=\"Level\" type=\"Node2D\"]\n\n<<<<<<<"), "{written}");
}

/// The README and the pre-commit hook both promise which findings stop a commit
/// and which do not, so the split is pinned here rather than left to drift.
#[test]
fn check_warns_without_failing_on_what_godot_would_still_load() {
    let s = Scratch::new("check-severity");

    // A stale load_steps: wrong, and Godot recomputes it on the next save.
    let stale = s.write(
        "stale.tscn",
        "[gd_scene load_steps=9 format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n",
    );
    let out = gdmerge(&["check", stale.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).contains("warning: load_steps is 9"), "{}", stdout(&out));
    assert!(stdout(&out).contains("0 failed"), "{}", stdout(&out));

    // A path that cannot be judged from this file: unverifiable, not wrong.
    let unverifiable = s.write(
        "instanced.tscn",
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"PackedScene\" path=\"res://a.tscn\" id=\"1\"]\n\n\
         [node name=\"Root\" type=\"Node2D\"]\n\n\
         [node name=\"Sub\" parent=\".\" instance=ExtResource(\"1\")]\n\n\
         [node name=\"W\" type=\"Node\" parent=\".\"]\n\
         p = NodePath(\"../Sub/Inner\")\n",
    );
    let out = gdmerge(&["check", unverifiable.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).contains("cannot be checked here"), "{}", stdout(&out));

    // A path that names nothing is the other side of the line: it fails.
    let broken = s.write(
        "broken.tscn",
        "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nx = NodePath(\"Nope\")\n",
    );
    let out = gdmerge(&["check", broken.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).contains("1 failed"), "{}", stdout(&out));
}
