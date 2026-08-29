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
}
