# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `gdmerge check` now validates every `NodePath` in a file: property values, animation track paths
  inside sub-resources, exported node paths, `%unique` names, the `path:subname` form, connection
  endpoints and `[editable]` paths. Each is resolved relative to whatever holds it. A path that
  resolves to nothing is an error; one that reaches into an instanced scene, or uses a unique name
  the file does not declare while instancing a scene that could supply it, is a warning.

### Changed

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

[Unreleased]: https://github.com/hyprtuna/gdmerge/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.2.1
[0.2.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.2.0
[0.1.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.1.0
