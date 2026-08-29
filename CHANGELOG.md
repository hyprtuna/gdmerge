# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/hyprtuna/gdmerge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hyprtuna/gdmerge/releases/tag/v0.1.0
