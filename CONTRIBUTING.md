# Contributing

Thanks for taking a look. Bug reports with the three files attached (the common ancestor, your
version and theirs) are the single most useful thing you can send.

## Development setup

```console
$ git clone https://github.com/hyprtuna/gdmerge
$ cd gdmerge
$ cargo build
$ cargo test
```

Rust 1.85 or newer. `git` must be on `PATH`: part of the suite drives a real `git merge` through the
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
| `docs/demo.sh` | The README's two examples, runnable, plus the renderer for `docs/demo.svg` and `docs/conflict.svg`. |

The design that everything else rests on: **parsing keeps every byte**. A value is stored as its
exact source text plus the byte ranges of the resource ids inside it, so serialising an unmodified
document reproduces the input exactly, and rewriting an id is a splice rather than a re-render. The
parsed value tree is used for *comparison only*, never for output.

## Fuzzing

`fuzz/` holds three libFuzzer targets: `lex` for the tokenizer, `value` for the variant
parser, and `document` for a whole file plus the byte-exact round trip. It is a separate
workspace, excluded from the root one, because it needs nightly and a sanitizer.

```console
$ cargo install cargo-fuzz
$ mkdir -p fuzz/corpus/document
$ cargo +nightly fuzz run document fuzz/corpus/document fuzz/seeds/document \
    crates/tscn/tests/fixtures -- -max_total_time=300
```

`fuzz/seeds/` is committed and small. `fuzz/corpus/` is what the fuzzer grows and is not
tracked; the real scene fixtures are passed as an extra read-only corpus instead of being
copied. A weekly workflow runs each target for ten minutes and uploads anything it finds.

If a run stops on a crash, minimise it with `cargo +nightly fuzz tmin <target> <artifact>`,
turn it into a test in `crates/tscn/tests/grammar.rs`, then fix it.

## The README's demos

`docs/demo.svg` and `docs/conflict.svg` are rendered from real runs, not drawn by hand. If you
change anything they show, regenerate them in the same commit:

```console
$ ./docs/demo.sh clean --svg docs/demo.svg
$ ./docs/demo.sh conflict --svg docs/conflict.svg
```

Run `./docs/demo.sh` or `./docs/demo.sh conflict` with no `--svg` to see the transcript first.

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
`.tscn`, or all `.tres`), the `expected` output, and, when the merge is meant to conflict, a
`conflicts.txt` with one entity description per line.

```console
$ mkdir crates/tscn/tests/merge_cases/21_my_case
$ # write base.tscn, ours.tscn and theirs.tscn
$ GDMERGE_BLESS=1 cargo test -p tscn --test merge_golden
$ git diff crates/tscn/tests/merge_cases/
```

`GDMERGE_BLESS=1` records whatever the current code produces, so **read the recorded output before
committing it**: it is a proposal, not an answer. Then run `cargo test` without the variable to
confirm it is stable.

Every case is also held to three invariants automatically, which is most of the value of adding one:
the merged output must parse and pass `gdmerge check`; swapping `ours` and `theirs` must not change
whether the merge is clean; and merging against an unchanged branch must return the other side byte
for byte. The same cases then run through the binary (`crates/gdmerge/tests/golden_cli.rs`), which
sends any input that fails `check` to a text merge instead, so all three inputs have to be valid
scenes: a case the tool would refuse records an answer nobody can reach.

## Changing merge behaviour

Behaviour changes need a golden case that fails before the change and passes after it. Re-record the
existing cases in the same commit and explain in the pull request why each changed output is better.
If a case's output gets *worse*, that is the change telling you something.

The grammar follows Godot's own `core/variant/variant_parser.cpp` and
`scene/resources/resource_format_text.cpp` on the 4.x branch. When something about the file format
is in question, those files are the authority; please do not guess, and cite what you found.

## Releasing

Only the repository owner cuts releases, but it is worth knowing what happens.

1. Bump the workspace version in the root `Cargo.toml`, run a build so `Cargo.lock` follows,
   and move the `Unreleased` entries in `CHANGELOG.md` under the new version.
2. Merge that through a pull request like anything else.
3. Push a `v<version>` tag. Nothing publishes on merge; the tag is what starts a release.
4. The workflow builds the binaries and creates the GitHub release, then waits. Publishing to
   crates.io needs an approval on the `release` environment, and uses trusted publishing, so
   there is no registry token anywhere.

`.github/scripts/publish-plan.py` decides what is left to publish and refuses two things
outright: a version older than one already on crates.io, and a release where every crate is
already at that version. Both mean an old or finished release is being replayed, so they fail
loudly instead of uploading something wrong or passing quietly. A release that got half way,
where one crate published and the next did not, still completes.

You can run that check yourself from a clean tree:

```console
$ python3 .github/scripts/publish-plan.py tscn gdmerge
```

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `test:`,
  `ci:`, `chore:`, `refactor:`, `perf:`.
- One logical change per commit. Rebase away fixup commits before opening the pull request.
- Update `CHANGELOG.md` under `## [Unreleased]` for anything user-visible.
- `main` is protected: everything lands through a pull request, CI must be green, and the repository
  owner merges.
