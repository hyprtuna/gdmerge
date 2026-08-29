# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3] - 2026-08-29

### Changed

- A file carrying a reference that was broken before the merge is handed to git's text merge, and
  the notes now say so. Since 0.3.2 every input has to pass the validation `gdmerge check` runs
  before the semantic merge touches it, and a `NodePath` that names nothing fails that validation,
  so such a file goes to `git merge-file` whole, with git's exit status, as the README's table
  says. The 0.3.0 "Upgrading" note claimed a reference that was already broken was passed through
  and did not block anything; that stopped being true in 0.3.2, and the note now carries a
  correction. The test guarding the old claim called the library directly, which does still pass
  an inherited break through; it now exercises the command and asserts the fallback. Running
  `gdmerge check` on the file and fixing what it reports gets the semantic merge back for it.
- The text fallback follows `merge.conflictstyle`. `git merge-file` was asked for `diff3` markers
  whatever style the user had configured; it is now left to read the setting itself, which is what
  the driver's missing-binary path and git's own merges already did. gdmerge's own markers are
  always the two-sided form, and the README now says so; `gdmerge mergetool` is what shows the base
  value of every property.
- The README documents `check --json`, with a sample, next to `diff --json`.

### Fixed

- `-O` can point at something that is not a regular file, such as `/dev/null` from a script that
  only wants the exit status. The result is normally written to a temporary file beside the target
  and renamed into place, which for `/dev/null` failed with a bare "Permission denied" from `/dev`.
  Such a target is now written directly, and when the temporary file cannot be created the error
  says that the directory beside the target has to be writable, and why.

- The conflict report shows the ids the merged file has. A sub-resource used from more than one
  node and edited on both branches survives as two sub-resources, theirs under a new id, with a
  conflict at each node referencing it. The stderr line for that conflict printed the same value
  for both sides, `ours SubResource("1_s") / theirs SubResource("1_s")`, and `gdmerge mergetool`
  showed the same in its table, because both were composed before the ids were assigned; the
  markers in the file showed the real difference. The two sides are now rendered the way the file
  renders them, so the report names the renumbered id.
- `gdmerge check` now catches two siblings that share an `index`. The rule only read the bare
  spelling (`index=0`), and Godot writes the field quoted (`index="0"`), so the collision it was
  meant to report never was: a file with two `parent="." index="0"` nodes passed `check`, and the
  pre-commit hook let it through. Both spellings are read now, and a quoted and a bare index that
  name the same slot collide with each other, as they do in Godot's loader. This makes the README's
  list of what `check` fails on true of colliding sibling indices, which it was not.

## [0.3.2] - 2026-08-29

### Changed

- The README's sub-resource passages describe what 0.3.0 shipped. The comparison table and the
  limitations list still said a sub-resource changed on both branches produced two sub-resources and
  a conflict at the referencing node. Since 0.3.0 sub-resources are matched by where they are used as
  well as by content: edits to different properties merge into one sub-resource, and edits to the
  same property conflict on the `[sub_resource]` itself. What remains true, and is now what the
  limitation says, is that a sub-resource referenced from more than one node, or from none, is still
  matched by its contents alone.
- The README and the pre-commit hook description now separate what `gdmerge check` fails on from
  what it only warns about. A stale `load_steps` was listed among the things `check` validates,
  which read as a promise that it would fail the check and so block a commit through the hook. It
  is a warning, and has been since 0.1.0: Godot recomputes `load_steps` on the next save. Nothing
  about the tool's behaviour changed, only what is claimed about it.

### Fixed

- Godot 3 files are no longer merged silently wrong. A scene whose resource ids use the Godot 3
  unquoted spelling (`id=1`, referenced as `SubResource( 1 )`) parses, so 0.1.0 through 0.3.1 merged
  it semantically: the `[sub_resource]` id was rewritten into the Godot 4 form while the references
  to it were left as they were, producing a scene that gdmerge and git both called clean and Godot
  could not load. Through the installed merge driver that ended as a committed, broken scene. Every
  input is now put through the same validation `gdmerge check` runs, and a file that fails it is
  handed to `git merge-file` like an unparseable one: byte-identical output, git's exit status, and
  one line on stderr naming the file and the reason. `gdmerge mergetool` applies the same guard,
  since it redoes the merge and overwrites the conflicted file.
- A merge that strands a `NodePath` now writes conflict markers. It was reported on stderr and by
  `check`, but the merged file itself came back with nothing in it to stop a commit, and
  `gdmerge mergetool` printed an empty table for it. The conflict is now raised at the entity holding
  the path: markers wrap that section, and the two sides of the item at fault are laid out like any
  other conflict, with the stranded path named. This makes the README's "conflict markers wrap the
  affected section" true of this case as well.
- A missing `gdmerge` binary no longer loses one side of a merge. With the driver configured and
  the binary not on the `PATH` git sees, git left the file conflicted holding only our side, with no
  conflict markers in it, so `git add` discarded the other side without a word. `gdmerge git-install`
  now writes the driver as a shell fragment that checks for the binary and runs
  `git merge-file` when it is absent, making the worst case git's own text merge. Existing
  installations pick this up by re-running `gdmerge git-install`.
- `gdmerge check` recognises the Godot 3 unquoted forms. `ExtResource( 1 )` and `SubResource( 1 )`
  are now resolved like their quoted counterparts, so a dangling reference written that way is
  reported instead of ignored, and a resource declaring `id=1` is reported as the legacy form rather
  than as having no id at all.

## [0.3.1] - 2026-08-29

The crates are byte for byte identical to 0.3.0. This release exists so the pre-commit hook has
a tag to pin: pre-commit reads `.pre-commit-hooks.yaml` from the revision you name, and that file
did not exist at `v0.3.0`.

### Added

- A [pre-commit](https://pre-commit.com) hook, `gdmerge-check`, so changed `.tscn` and `.tres` files
  are validated before they are committed. It uses the `gdmerge` on your `PATH` rather than building
  one, since anyone using the merge driver already has it.

## [0.3.0] - 2026-08-29

### Upgrading

Merges that used to succeed can now conflict, deliberately. gdmerge validates its own result
and refuses to hand back a scene that is wired to a node which is not there. In practice most
renames improve rather than conflict, because the references now follow the rename; what
conflicts is the case where following them is impossible, such as one branch deleting a node
the other branch started referencing. A reference that was already broken before the merge is
passed through as before, so existing breakage does not block anything.
Correction: the last sentence stopped being true in 0.3.2, which hands such a file to git's text
merge; see the 0.3.3 entry.

### Added

- `gdmerge check` now validates every `NodePath` in a file: property values, animation track paths
  inside sub-resources, exported node paths, `%unique` names, the `path:subname` form, connection
  endpoints and `[editable]` paths. Each is resolved relative to whatever holds it. A path that
  resolves to nothing is an error; one that reaches into an instanced scene, or uses a unique name
  the file does not declare while instancing a scene that could supply it, is a warning.

### Changed

- Sub-resources are matched by where they are used as well as by their contents. A sub-resource
  edited on both branches used to stop looking like one thing, so it was duplicated and the node
  referencing it conflicted. It is now recognised as a single entity: edits to different properties
  merge, and edits to the same property conflict on the sub-resource itself, naming the property.
  Where a sub-resource is used from more than one place, or from none, its contents still identify
  it.
- `NodePath` values now follow a rename or reparent. A path is resolved against the scene tree from
  the node holding it and written out again naming the same node, in property values, animation
  track paths inside sub-resources, exported node paths, the `path:subname` form and `%unique`
  names. Paths are never matched as text, so `Player` is not confused with `PlayerCamera`. Anything
  whose meaning is not certain is left alone: paths into instanced scenes, paths above the root, and
  paths that read differently depending on which node they are measured from.
- A merge whose result would strand a `NodePath` is reported as a conflict naming the path, instead
  of succeeding. A reference that was already broken in the common ancestor passes through
  untouched, since it is not the merge's doing; `check` still reports it.

### Fixed

- Merging is deterministic again. Sub-resource resolution iterated a hash set, so which base a path
  was reported against, and occasionally the merged output itself, could differ between runs.

## [0.2.1] - 2026-08-29

### Fixed

- A file that nests values thousands of levels deep no longer overflows the stack and aborts the
  process. Nesting is parsed recursively and had no limit; values may now nest 128 levels, which is
  far more than a real scene uses and more than Godot's own writer will emit, and anything deeper is
  a parse error like any other malformed input. Found by fuzzing. This matters because gdmerge reads
  files that arrive from forks and pull requests.

### Added

- A timing guard over the largest fixture, so an accidentally quadratic parser or merge shows up as
  a test failure rather than as a slow tool.
- Fuzz targets for the tokenizer, the variant parser and whole-document parsing, in `fuzz/`, with a
  weekly workflow that runs each for ten minutes.

## [0.2.0] - 2026-08-29

### Added

- Renames and reparents are now tracked through a merge. A node renamed or moved on one branch and
  edited on the other merges cleanly instead of conflicting, and the rename carries the node's
  children, its connection endpoints and its `[editable]` entries with it. A node added under a
  subtree the other branch renamed is reparented onto the new path. Renaming one node to two
  different names on the two branches is reported as a conflict, with the reason named.
- `Conflict` now carries the header field or property that could not be reconciled, in a new `key`
  field, and its `detail` names it.
- `gdmerge mergetool`, a new subcommand that redoes a conflicted merge and prints the conflicting
  node one property per row, with the base, ours and theirs values side by side and the rows that
  actually disagree marked. `gdmerge git-install` registers it as a git mergetool, so
  `git mergetool --tool=gdmerge` works after the usual one command setup.
- The merge driver now names the items that disagreed on stderr instead of only saying which entity
  conflicted.
- `Conflict` carries a `rows` field with every item of the entity as each of the three sides has it,
  which is what both of those renderings are built from.

### Changed

- The declared minimum Rust version is now accurate and checked in CI. `tscn` builds on 1.74 as
  before; `gdmerge` declares 1.85, which is what its dependency tree actually needs. The old
  workspace wide claim of 1.74 for both was wrong.
- `Conflict` gained the `key` and `rows` fields. Code that constructs a `Conflict` has to be
  updated; code that only reads one does not.

### Fixed

- A merge of a file that uses CRLF line endings no longer emits LF for the blank
  lines it inserts between sections, which produced a file with mixed line
  endings on a Windows checkout with `core.autocrlf=true`.

## [0.1.0] - 2026-08-29

First release.

### Added

- `gdmerge merge`: three-way merge of `.tscn` and `.tres` files by semantic identity. Nodes are
  matched by scene-tree path, external resources by `uid` then `path`, sub-resources by content, and
  connections by their endpoints, so randomised per-file resource ids no longer cause conflicts.
  Colliding ids are reassigned and every reference to them rewritten; `load_steps` is recomputed.
  Conflict markers wrap only the entity that actually conflicts. Accepts git's merge-driver argument
  order (`%O %A %B %L %P`), so it works as a driver as well as from the command line.
- `gdmerge diff`: semantic diff reporting added, removed, renamed, reparented, reordered and
  modified nodes, resources and connections, with `--json` output.
- `gdmerge check`: parses a file, proves it round-trips byte for byte, and validates it: dangling
  `ExtResource`/`SubResource` references, duplicate ids, duplicate or orphaned node paths, missing or
  multiple roots, colliding sibling indices, and a stale `load_steps`.
- `gdmerge git-install` / `gdmerge git-uninstall`: one-command setup and removal of the git merge
  driver and the `.gitattributes` entries, per repository or per user.
- `tscn`, the library behind the tool: a lossless parser and serializer for the Godot 4 text
  resource format, plus the semantic model, diff, merge and checks.

### Safety

- A merge that would produce anything other than a clean, re-parseable file is abandoned in favour
  of `git merge-file`, whose exit status is passed through.
- Output is written to a temporary file and renamed into place, so a partial file is never left
  behind.
- A branch that made no semantic change never has its bytes rewritten by the other branch.

[Unreleased]: https://github.com/hyprtuna/gdmerge/compare/v0.3.3...HEAD
[0.3.3]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.3.3
[0.3.2]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.3.2
[0.3.1]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.3.1
[0.3.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.3.0
[0.2.1]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.2.1
[0.2.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.2.0
[0.1.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.1.0
