# Contributing

Thanks for taking a look. Bug reports with the three files attached — the common ancestor, your
version and theirs — are the single most useful thing you can send.

## Development setup

```console
$ git clone https://github.com/hyprtuna/gdmerge
$ cd gdmerge
$ cargo build
$ cargo test
```

Rust 1.74 or newer. `git` must be on `PATH`: part of the suite drives a real `git merge` through the
installed driver.

Before pushing, run what CI runs:

```console
$ cargo fmt --all -- --check
$ cargo clippy --all-targets --all-features -- -D warnings
$ cargo test --all-features --workspace
```

## Layout

| Path | What lives there |
| --- | --- |
| `crates/tscn` | The library: lexer, lossless document model, semantic identities, diff, merge, checks. No I/O. |
| `crates/gdmerge` | The command-line tool: argument handling, file I/O, git configuration, text fallback. |
| `crates/tscn/tests/fixtures` | Real scene files from godotengine/godot-demo-projects. |
| `crates/tscn/tests/merge_cases` | Golden three-way merge cases. |
| `docs/demo.sh` | The README's example, runnable, plus the renderer for `docs/demo.svg`. |

The design that everything else rests on: **parsing keeps every byte**. A value is stored as its
exact source text plus the byte ranges of the resource ids inside it, so serialising an unmodified
document reproduces the input exactly, and rewriting an id is a splice rather than a re-render. The
parsed value tree is used for *comparison only*, never for output.

## Adding a fixture

Fixtures are unmodified files from
[godotengine/godot-demo-projects](https://github.com/godotengine/godot-demo-projects) (MIT). To add
one:

1. Copy it, byte for byte, into `crates/tscn/tests/fixtures/` under a flattened snake_case name.
2. Add a row to the table in `crates/tscn/tests/fixtures/ATTRIBUTION.md` mapping the new name to its
   upstream path.
3. `cargo test -p tscn --test fixtures_roundtrip`.

Add a fixture when it exercises syntax the corpus does not already cover. If the file comes from
somewhere other than the demo projects, say where and under what license in the pull request; a
license-compatible file with clear provenance is fine, an ad-hoc copy of someone's game is not.

## Adding a golden merge case

Each directory under `crates/tscn/tests/merge_cases/` holds `base`, `ours` and `theirs` (all
`.tscn`, or all `.tres`), the `expected` output, and — when the merge is meant to conflict — a
`conflicts.txt` with one entity description per line.

```console
$ mkdir crates/tscn/tests/merge_cases/21_my_case
$ # write base.tscn, ours.tscn and theirs.tscn
$ GDMERGE_BLESS=1 cargo test -p tscn --test merge_golden
$ git diff crates/tscn/tests/merge_cases/
```

`GDMERGE_BLESS=1` records whatever the current code produces, so **read the recorded output before
committing it** — it is a proposal, not an answer. Then run `cargo test` without the variable to
confirm it is stable.

Every case is also held to three invariants automatically, which is most of the value of adding one:
the merged output must parse and pass `gdmerge check`; swapping `ours` and `theirs` must not change
whether the merge is clean; and merging against an unchanged branch must return the other side byte
for byte.

## Changing merge behaviour

Behaviour changes need a golden case that fails before the change and passes after it. Re-record the
existing cases in the same commit and explain in the pull request why each changed output is better.
If a case's output gets *worse*, that is the change telling you something.

The grammar follows Godot's own `core/variant/variant_parser.cpp` and
`scene/resources/resource_format_text.cpp` on the 4.x branch. When something about the file format
is in question, those files are the authority — please do not guess, and cite what you found.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `test:`,
  `ci:`, `chore:`, `refactor:`, `perf:`.
- One logical change per commit. Rebase away fixup commits before opening the pull request.
- Update `CHANGELOG.md` under `## [Unreleased]` for anything user-visible.
- `main` is protected: everything lands through a pull request, CI must be green, and the repository
  owner merges.
