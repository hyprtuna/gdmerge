# gdmerge

Semantic diff and 3-way merge for Godot 4 scenes and resources (`.tscn` / `.tres`): a git
merge driver that ends scene merge conflicts.

Godot mints a fresh random id for every resource in a scene file, per file, on every save. Two
teammates who each add one texture will both get an id like `2_ni7xa`, on the same line, and git,
which only sees text, declares a conflict. gdmerge reads the file the way Godot does, merges
nodes, resources and connections by *identity* rather than by line number, and reassigns the ids
itself.

## Setup

```console
$ cargo install gdmerge
$ cd my-godot-project
$ gdmerge git-install
```

That is all of it. `git merge`, `git rebase`, `git cherry-pick` and `git stash pop` start using it
for `*.tscn` and `*.tres`. Commit the `.gitattributes` it writes; each teammate runs
`gdmerge git-install` once so their own git knows what the driver is.

## Commands

| Command | What it does |
| --- | --- |
| `gdmerge merge --base O --ours A --theirs B` | Three-way merge. Exit `0` clean, `1` with conflicts. Also accepts git's `%O %A %B %L %P` argument order. |
| `gdmerge diff A B` | Semantic diff: nodes, resources and connections added, removed, moved or changed. `--json` available. |
| `gdmerge check FILE...` | Parse, prove a byte-exact round trip, and validate structurally. Useful as a pre-commit hook. |
| `gdmerge git-install [--global]` | Register the merge driver and the `.gitattributes` entries. |

Disjoint changes merge automatically: nodes added on both branches, different properties of one
node, resources that collided on an id, a reorder against an edit. Genuine collisions (the same
property set two ways, a delete against a modify) conflict, with markers around just that node.

A file gdmerge cannot parse is handed to `git merge-file`, so it is never worse than not having it
installed. Output is written atomically, and a clean merge is re-parsed before it is accepted.

Full documentation, the comparison table, and the list of limitations are in the
[repository README](https://github.com/hyprtuna/gdmerge#readme).

## License

MIT.
