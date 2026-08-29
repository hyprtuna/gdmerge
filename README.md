# gdmerge

Semantic diff and 3-way merge for Godot 4 scenes and resources (`.tscn` / `.tres`): a git
merge driver that ends scene merge conflicts.

Godot mints a fresh random id for every resource in a scene file, per file, on every save. Two
teammates who each add one texture will both get an id like `2_ni7xa`, on the same line, and git,
which only sees text, declares a conflict that a human then has to resolve by hand in a file
format that punishes mistakes. gdmerge reads the file the way Godot does, merges nodes, resources
and connections by *identity* instead of by line number, and reassigns the ids itself.

![A git merge that conflicts, then the same merge succeeding once gdmerge is installed](docs/demo.svg)

## 30-second setup

```console
$ cargo install gdmerge          # or grab a binary from the Releases page
$ cd my-godot-project
$ gdmerge git-install
```

That is the whole setup. `git merge`, `git rebase`, `git cherry-pick` and `git stash pop` all start
using it for `*.tscn` and `*.tres`. Commit the `.gitattributes` it writes; each teammate runs
`gdmerge git-install` once so their own git knows what the `gdmerge` driver is.

See [INSTALL.md](INSTALL.md) for prebuilt binaries, global setup, and how to verify the driver is
active.

## What it actually does

Start from a level with one texture. You add a player sprite; a teammate adds a footstep sound.
Godot gave both new resources the id `2_ni7xa`.

**What git does on its own** (`git merge-file --diff3`, exit status 2):

```tscn
[gd_scene load_steps=3 format=3 uid="uid://demo_level"]

[ext_resource type="Texture2D" uid="uid://tex_ground" path="res://ground.png" id="1_ground"]
<<<<<<< ours
[ext_resource type="Texture2D" uid="uid://tex_player" path="res://player.png" id="2_ni7xa"]
||||||| base
=======
[ext_resource type="AudioStream" uid="uid://snd_step" path="res://step.ogg" id="2_ni7xa"]
>>>>>>> theirs

[node name="Level" type="Node2D"]

[node name="Ground" type="Sprite2D" parent="."]
texture = ExtResource("1_ground")
<<<<<<< ours

[node name="Player" type="Sprite2D" parent="."]
texture = ExtResource("2_ni7xa")
||||||| base
=======

[node name="Steps" type="AudioStreamPlayer" parent="."]
stream = ExtResource("2_ni7xa")
>>>>>>> theirs
```

**What gdmerge does** (exit status 0):

```tscn
[gd_scene load_steps=4 format=3 uid="uid://demo_level"]

[ext_resource type="Texture2D" uid="uid://tex_ground" path="res://ground.png" id="1_ground"]
[ext_resource type="AudioStream" uid="uid://snd_step" path="res://step.ogg" id="2_gdm0"]
[ext_resource type="Texture2D" uid="uid://tex_player" path="res://player.png" id="2_ni7xa"]

[node name="Level" type="Node2D"]

[node name="Ground" type="Sprite2D" parent="."]
texture = ExtResource("1_ground")

[node name="Steps" type="AudioStreamPlayer" parent="."]
stream = ExtResource("2_gdm0")

[node name="Player" type="Sprite2D" parent="."]
texture = ExtResource("2_ni7xa")
```

Both resources survive, the colliding id is reassigned, every reference to it is rewritten, and
`load_steps` is recomputed. Our side's ids are never renumbered; only an incoming resource whose id
would collide gets a new one.

## What is resolved and what conflicts

| Situation | Result |
| --- | --- |
| Each branch adds a different node | both kept |
| Each branch adds a different resource that got the same id | both kept, one id reassigned, references rewritten |
| Each branch edits a *different* property of the same node | both edits applied |
| Each branch edits a different node | both edits applied |
| One branch reorders nodes, the other edits one of them | our order kept, their edit applied |
| One branch renames or reparents a node, the other edits it | both applied: the node keeps the new path and gains the edit |
| A renamed node's children, connections and `[editable]` entries | follow the rename automatically |
| A node added under a subtree the other branch renamed | reparented onto the new path |
| One branch deletes a node the other did not touch | deleted |
| Both branches add the same node with the same contents | one copy |
| Both branches reference one resource under different ids | matched by `uid`, then by `path`; one entry |
| Sub-resources with identical contents but different ids | matched by content; one entry |
| `load_steps` disagreeing between branches | recomputed from the merged file |
| Both branches set the *same* property to different values | **conflict**, markers around that node only |
| Both branches rename one node to different names | **conflict**, markers around that node only |
| One branch deletes a node the other edited | **conflict**, markers around that node only |
| Both branches change one sub-resource differently | both kept; **conflict** at the node that references it |
| A file gdmerge cannot parse | hands the whole merge to `git merge-file` and returns its exit status |

Conflict markers wrap the affected `[node]` or `[sub_resource]` section, not the whole file, so the
rest of the scene stays readable and the part you need to look at is obvious.

When a merge does conflict, the driver says what about, on stderr:

```console
gdmerge: conflict in root node "Player" (speed changed differently on both sides)
gdmerge:   speed: ours 250.0 / theirs 400.0
gdmerge: run `git mergetool --tool=gdmerge` to see the two sides side by side
```

and `git mergetool --tool=gdmerge` lays the whole node out, one property per row:

![A conflicting merge, then git mergetool showing the base, ours and theirs values of every property on the conflicting node](docs/conflict.svg)

```console
$ git mergetool --tool=gdmerge
1 conflict in level.tscn

Conflict 1 of 1: root node "Player"
  speed changed differently on both sides

     property  base               ours               theirs
     --------  -----------------  -----------------  -----------------
     name      "Player"           "Player"           "Player"
     type      "CharacterBody2D"  "CharacterBody2D"  "CharacterBody2D"
  >  speed     100.0              250.0              400.0

Rows marked with > are the ones to resolve. Edit the file, remove the conflict
markers, then stage it.
```

`gdmerge git-install` registers the mergetool along with the driver. To reach for it without
typing `--tool` every time, `git config merge.tool gdmerge`.

## Safety

- **Never loses data.** A merge that produces anything other than a clean, re-parseable file is
  abandoned in favour of git's own text merge.
- **Never writes partial output.** Results are written to a temporary file and renamed into place.
- **Never rewrites an untouched branch.** If one side made no semantic change, the other side's
  bytes are returned exactly as they were: byte for byte, comments and formatting included.
- **Fails loud.** Exit `0` for a clean merge, `1` for conflicts, `2` for an error; conflicts are
  named on stderr.

## The other two commands

`gdmerge diff` shows what changed in terms Godot users think in (nodes, resources, connections)
instead of lines:

```console
$ gdmerge diff base.tscn ours.tscn
2 semantic changes

  + ext_resource uid://tex_player

  + node Player
```

Add `--json` for a machine-readable form.

`gdmerge mergetool` is what `git mergetool --tool=gdmerge` runs. It redoes the merge from the three
pristine versions git hands it, writes the result, and prints the table above. It also works
directly: `gdmerge mergetool base.tscn ours.tscn theirs.tscn out.tscn`.

`gdmerge check` parses a file, proves it round-trips byte for byte, and validates it structurally:
dangling `ExtResource`/`SubResource` references, duplicate ids, duplicate or orphaned node paths,
more than one root, colliding sibling indices, and a stale `load_steps`. It is useful on its own as
a pre-commit hook:

```console
$ gdmerge check level.tscn
ok   level.tscn

1 file checked, 0 failed
```

## Limitations

- **A rename combined with an edit, in the same branch, is not tracked.** Nodes are matched across
  a rename by their contents, so a branch that renames a node *and* changes it in one step no
  longer matches, and you get a delete against a modify. A rename on one branch and an edit on the
  other merges cleanly, which is the common case.
- **Node paths inside property values are not rewritten.** A rename updates `parent`, connection
  endpoints and `[editable]` paths, but a `NodePath("Old/Thing")` sitting in some property keeps
  its old text.
- **Reordering is not merged element-wise.** If both branches reorder the same siblings, our order
  wins rather than the two being interleaved.
- **`load_steps` is only recomputed when it is already present.** Godot omits it in many saved
  files, and adding it back would create noise in every merged file.
- **Godot 4 text formats only.** No `.escn`, no binary `.scn` / `.res`, no Godot 3 files. Those fall
  through to git's text merge.
- **No GUI and no editor plugin.** It is a command-line merge driver.
- Sub-resources are matched by content, so *changing* one on both branches produces two
  sub-resources plus a conflict at the referencing node, rather than a merge of the two.

## Compared to the alternatives

| | Editing conflicts by hand | [derkork/tscnmerge](https://github.com/derkork/tscnmerge) | Unity's UnityYAMLMerge | gdmerge |
| --- | --- | --- | --- | --- |
| Godot 4 support | n/a | no (Godot 3) | no (Unity only) | yes |
| Maintained | n/a | archived | yes | yes |
| Runs as a git merge driver | no | yes | yes | yes |
| Handles randomised resource ids | manually | Godot 3 ids only | n/a | yes |
| Semantic diff command | no | no | no | yes |
| Structural validation command | no | no | no | yes |
| Install | n/a | Python + pip | ships with Unity | single binary |

If you use Unity, UnityYAMLMerge already does this for you and has for years. Godot has had nothing
maintained; that is the gap this fills.

## FAQ

**Do my teammates need it installed?**
Yes. `.gitattributes` records *which* files use the `gdmerge` driver and is committed; the driver
itself is defined in each person's git config by `gdmerge git-install`. Anyone who has not installed
it just gets git's normal text merge, exactly as before.

**Will it reformat my scene files?**
No. Sections are replayed from the original bytes, including comments and whitespace. Only the
blank lines *between* sections are normalised, and only when a merge actually combines both sides.

**Why did some resource ids change?**
Only when two branches independently used the same id for different resources. One of them has to
move; gdmerge keeps ours and reassigns theirs, rewriting every reference. Godot will renumber
everything to its own scheme the next time it saves the scene anyway.

**Does it work with rebase and cherry-pick?**
Yes. Anything that goes through git's merge machinery, including `git rebase`, `git cherry-pick`,
`git stash pop` and `git revert`.

**What if it hits a file it does not understand?**
It says so on stderr and hands that merge to `git merge-file`, returning git's exit status. You are
never worse off than without it installed.

**Is the parser trustworthy?**
Its grammar follows Godot's own `VariantParser` and `ResourceLoaderText`. During development it was
run over 814 `.tscn` and `.tres` files from public Godot 4 projects; all 814 parsed and re-serialised
byte for byte. Thirty of those files ship as fixtures, and the byte-exact round trip over them is a
hard gate in CI.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports are much easier to act on with the three files
attached: the common ancestor, your version, and theirs.

## License

MIT. See [LICENSE](LICENSE).
