# Installing gdmerge

## 1. Get the binary

### Prebuilt binaries

Download the archive for your platform from the
[Releases page](https://github.com/hyprtuna/gdmerge/releases), verify it against the `.sha256` file
published beside it, extract it, and put `gdmerge` somewhere on your `PATH`.

| Platform | Archive |
| --- | --- |
| Linux, x86-64 | `gdmerge-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS, Apple silicon | `gdmerge-<version>-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `gdmerge-<version>-x86_64-apple-darwin.tar.gz` |
| Windows, x86-64 | `gdmerge-<version>-x86_64-pc-windows-msvc.zip` |

```console
$ tar xzf gdmerge-*-x86_64-unknown-linux-gnu.tar.gz
$ install -m755 gdmerge ~/.local/bin/gdmerge
```

On macOS the binaries are unsigned, so the first run may need
`xattr -d com.apple.quarantine ./gdmerge`.

### From crates.io

```console
$ cargo install gdmerge
```

### From source

```console
$ git clone https://github.com/hyprtuna/gdmerge
$ cd gdmerge
$ cargo build --release
$ install -m755 target/release/gdmerge ~/.local/bin/gdmerge
```

Requires Rust 1.85 or newer for the tool. The `tscn` library alone builds on 1.74. `cargo test` runs the full suite, including a real `git merge` through
the installed driver, so `git` needs to be on `PATH`.

## 2. Register the merge driver

Two separate things have to be in place before git will use a custom merge driver:

1. a **driver definition** in a git config file, which says what command to run, and
2. an **attributes entry** that points `*.tscn` and `*.tres` at that driver.

`gdmerge git-install` writes both.

### Per repository (recommended)

```console
$ cd my-godot-project
$ gdmerge git-install
```

This writes the driver into `.git/config` and appends to the repository's `.gitattributes`:

```gitattributes
# gdmerge
*.tscn merge=gdmerge
*.tres merge=gdmerge
```

**Commit that `.gitattributes`.** It is what tells every clone of the project that scene files use
this driver. Each teammate then runs `gdmerge git-install` once in their own clone so their git
knows what `gdmerge` means. Someone who skips it simply gets git's normal text merge.

### For your user account

```console
$ gdmerge git-install --global
```

This writes the driver into your global git config and the patterns into your global attributes
file (`core.attributesfile`, defaulting to `~/.config/git/attributes`, which is created and
registered if it does not exist). Every repository you touch is covered, including ones whose
`.gitattributes` you cannot change.

### Undoing it

```console
$ gdmerge git-uninstall          # this repository
$ gdmerge git-uninstall --global # your user account
```

This removes only the lines `git-install` added.

## 3. Verify it is active

Check the driver definition:

```console
$ git config --get merge.gdmerge.driver
if command -v gdmerge >/dev/null 2>&1; then gdmerge merge %O %A %B %L %P; else git merge-file -L ours -L base -L theirs %A %O %B; fi
```

git runs a merge driver through the shell, so the driver checks for the binary first and runs
git's own text merge if it is not there. That is what makes a missing `gdmerge` no worse than
never having installed it: see below.

Check that git will apply it to a scene file:

```console
$ git check-attr merge -- some/scene.tscn
some/scene.tscn: merge: gdmerge
```

If that prints `merge: unspecified`, the attributes entry is missing or the path does not match.

Finally, confirm the binary itself works:

```console
$ gdmerge --version
$ gdmerge check some/scene.tscn
```

## Using it without installing the driver

Every command works standalone, which is handy in CI:

```console
$ gdmerge check $(git ls-files '*.tscn' '*.tres')
$ gdmerge diff old.tscn new.tscn
$ gdmerge merge --base ancestor.tscn --ours mine.tscn --theirs yours.tscn -O merged.tscn
```

## As a pre-commit hook

If you use [pre-commit](https://pre-commit.com), add this to your project's
`.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/hyprtuna/gdmerge
    rev: v0.3.3
    hooks:
      - id: gdmerge-check
```

Every changed `.tscn` and `.tres` is then checked before it is committed, which catches a dangling
resource reference or a node path naming something that is not there at the point it is introduced,
rather than when somebody opens the scene and wonders why nothing moves.

The hook blocks the commit on anything Godot would refuse to load or would load wrongly, which is
what `gdmerge check` exits `1` for. It does not block on the things `check` only warns about: a
stale `load_steps`, which Godot recomputes the next time it saves the scene, and a `NodePath` that
cannot be judged from one file, such as one reaching into an instanced scene. Those are printed and
the commit goes through.

The hook runs the `gdmerge` already on your `PATH`; it does not build one. If you have run
`gdmerge git-install` you already have it. Otherwise install it as above first, or the hook will
report that the executable was not found.

## Troubleshooting

**`git merge` still conflicts.** Confirm both halves are in place with the two commands above. Note
that git only invokes a merge driver when both sides changed the file; if only one side changed it,
git takes that side without consulting any driver.

**`gdmerge: falling back to a text merge`.** One of the three inputs did not parse, or parsed and
failed the validation `gdmerge check` runs. The message names the file and the reason. A reference
that was already broken before the merge is enough, such as a `NodePath` naming a node that is not
there: a file the checks reject is one the semantic merge cannot reason about safely, so git's own
text merge runs instead, with its exit status, and nothing is lost. To get the semantic merge back
for that file, run `gdmerge check` on it, fix what it reports, and commit the fix; from then on
gdmerge merges it again. If the file parses and passes `check`, please open an issue with the three
files attached.

**gdmerge is not on `PATH` during a merge.** git runs the driver through your shell, so the binary
has to be on the `PATH` git sees, which is not always the one your interactive shell has. When it
is missing the driver falls back to `git merge-file` and you get git's ordinary text merge, with
conflict markers holding both sides: the same result as if `.gitattributes` had never named the
driver, and nothing is lost. To get the semantic merge back, either move the binary somewhere
already on `PATH`, or point the driver at an absolute path:

```console
$ git config merge.gdmerge.driver "/full/path/to/gdmerge merge %O %A %B %L %P"
```

Note that an absolute path spelled out this way gives up the fallback: if that path stops being
valid, git is back to leaving the file conflicted with only your side in it. Re-running
`gdmerge git-install` restores the guarded form.

**`git mergetool` ends with `merge of level.tscn failed`.** That is git's wording for a mergetool
that exited non-zero, and gdmerge exits `1` whenever conflicts remain, which after it has shown you
the table is the expected outcome. Nothing was lost: the file holds the conflict markers with both
sides, exactly as the table describes. Edit it, remove the markers, and `git add` it.
