#!/usr/bin/env bash
# Runs the README's examples for real, in a throwaway git repository.
#
#   ./docs/demo.sh                      the clean merge, printed
#   ./docs/demo.sh conflict             the conflict walkthrough, printed
#   ./docs/demo.sh clean --svg OUT.svg  also render the transcript to an SVG
#
# Everything below is a real git merge; nothing is simulated.

set -euo pipefail

invoked_from=$PWD
repo_root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
transcript=$work/transcript
trap 'rm -rf "$work"' EXIT

scenario=clean
case ${1-} in
clean | conflict)
	scenario=$1
	shift
	;;
esac

cargo build --release --quiet --manifest-path "$repo_root/Cargo.toml"
export PATH="$repo_root/target/release:$PATH"

# Nothing from the machine running this may reach the transcript: no global
# git config (a merge.conflictstyle there changes the markers git writes), no
# attributes file, no home directory of the author's.
export HOME=$work/home XDG_CONFIG_HOME=$work/home/.config
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=$work/home/.gitconfig
export GIT_AUTHOR_NAME=Demo GIT_AUTHOR_EMAIL=demo@example.invalid
export GIT_COMMITTER_NAME=Demo GIT_COMMITTER_EMAIL=demo@example.invalid
mkdir -p "$HOME"

# Runs the scenario in a fresh repository under $1, writing the transcript to $2.
record() {
	local demo=$1
	transcript=$2
	mkdir -p "$demo"
	cd "$demo"
	git init -q -b main .
	git config user.name Demo
	git config user.email demo@example.invalid
	case $scenario in
	clean) demo_clean ;;
	conflict) demo_conflict ;;
	esac
	# The scratch repository lives in a temporary directory; show a stable
	# path instead so the transcript reads the same wherever it is run.
	sed -i "s#$demo#/home/you/game#g" "$transcript"
}

say() { printf '$ %s\n' "$1" >>"$transcript"; }
out() { sed 's/^/  /' >>"$transcript"; }

# --- the clean merge: two branches, two new resources, one minted id ---------
scene() { # scene <load_steps> <extra-resource-line> <extra-node-block>
	cat <<EOF
[gd_scene load_steps=$1 format=3 uid="uid://demo_level"]

[ext_resource type="Texture2D" uid="uid://tex_ground" path="res://ground.png" id="1_ground"]
$2
[node name="Level" type="Node2D"]

[node name="Ground" type="Sprite2D" parent="."]
texture = ExtResource("1_ground")
$3
EOF
}

demo_clean() {
	scene 2 "" "" >level.tscn
	git add . && git commit -qm "add the level"

	# A teammate adds a footstep sound; the editor mints the id 2_ni7xa for it.
	git checkout -q -b audio
	scene 3 '[ext_resource type="AudioStream" uid="uid://snd_step" path="res://step.ogg" id="2_ni7xa"]' '
[node name="Steps" type="AudioStreamPlayer" parent="."]
stream = ExtResource("2_ni7xa")' >level.tscn
	git commit -qam "add footstep audio"

	# Meanwhile we add a player sprite, and the editor mints the same id for it.
	git checkout -q main
	scene 3 '[ext_resource type="Texture2D" uid="uid://tex_player" path="res://player.png" id="2_ni7xa"]' '
[node name="Player" type="Sprite2D" parent="."]
texture = ExtResource("2_ni7xa")' >level.tscn
	git commit -qam "add the player sprite"

	say "git merge audio"
	if git merge --no-edit audio >/dev/null 2>&1; then
		echo "merged cleanly (unexpected for this example)" | out
	else
		printf 'CONFLICT (content): Merge conflict in level.tscn\nAutomatic merge failed; fix conflicts and then commit the result.\n' | out
	fi
	grep -n '<<<<<<<\|=======\|>>>>>>>' level.tscn | head -3 | out
	git merge --abort

	say "gdmerge git-install"
	gdmerge git-install 2>&1 | head -2 | out

	say "git merge audio"
	git merge --no-edit audio 2>&1 | grep -v '^ ' | head -2 | out

	say "gdmerge diff <(git show HEAD^1:level.tscn) level.tscn"
	gdmerge diff <(git show 'HEAD^1:level.tscn') level.tscn | head -8 | out

	say "gdmerge check level.tscn"
	gdmerge check level.tscn | out
}

# --- the conflict: one property, set two ways, explained ---------------------
player() { # player <speed>
	cat <<EOF
[gd_scene format=3 uid="uid://demo_player"]

[node name="Player" type="CharacterBody2D"]
speed = $1
EOF
}

demo_conflict() {
	player 100.0 >player.tscn
	git add . && git commit -qm "add the player"
	gdmerge git-install >/dev/null

	git checkout -q -b tuning
	player 400.0 >player.tscn
	git commit -qam "make the player faster"

	git checkout -q main
	player 250.0 >player.tscn
	git commit -qam "tune the player speed"

	say "git merge tuning"
	git merge --no-edit tuning 2>&1 | grep -v '^Auto-merging' | head -5 | out || true

	say "git mergetool --no-prompt --tool=gdmerge"
	# Drop git's own preamble and keep what the tool itself printed.
	git mergetool --no-prompt --tool=gdmerge 2>/dev/null |
		sed -n '/{remote}/,$p' | tail -n +2 | head -14 | out || true
}

# Twice, and the two runs have to agree: a transcript that depends on anything
# but the tool is not one to publish.
record "$work/first" "$work/first.transcript"
record "$work/second" "$work/second.transcript"
if ! cmp -s "$work/first.transcript" "$work/second.transcript"; then
	echo "demo.sh: the transcript is not reproducible:" >&2
	diff "$work/first.transcript" "$work/second.transcript" >&2 || true
	exit 1
fi
transcript=$work/first.transcript

cat "$transcript"

if [ "${1-}" = "--svg" ]; then
	svg_out=${2:?usage: demo.sh [clean|conflict] --svg OUT.svg}
	case $svg_out in /*) ;; *) svg_out=$invoked_from/$svg_out ;; esac
	rows=$(wc -l <"$transcript")
	height=$((rows * 22 + 56))
	{
		printf '<svg xmlns="http://www.w3.org/2000/svg" width="860" height="%s" font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" font-size="14">\n' "$height"
		printf '<style>\n'
		printf '.bg{fill:#12141c}.p{fill:#7aa2f7}.o{fill:#c0caf5}.c{fill:#9ece6a}.w{fill:#e0af68}\n'
		printf 'text{opacity:0;animation:r .01s linear forwards}@keyframes r{to{opacity:1}}\n'
		printf '</style>\n'
		printf '<rect class="bg" width="860" height="%s" rx="8"/>\n' "$height"
		printf '<circle cx="22" cy="22" r="6" fill="#f7768e"/><circle cx="42" cy="22" r="6" fill="#e0af68"/><circle cx="62" cy="22" r="6" fill="#9ece6a"/>\n'
		n=0
		while IFS= read -r line; do
			y=$((n * 22 + 62))
			delay=$(awk "BEGIN{printf \"%.2f\", $n * 0.35}")
			case $line in
			'$ '*) cls=p ;;
			*'  >  '*) cls=w ;;
			*conflict* | *CONFLICT* | *'<<<'* | *'>>>'* | *'==='*) cls=w ;;
			*ok*) cls=c ;;
			*) cls=o ;;
			esac
			esc=$(printf '%s' "$line" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g')
			printf '<text class="%s" x="20" y="%s" style="animation-delay:%ss" xml:space="preserve">%s</text>\n' "$cls" "$y" "$delay" "$esc"
			n=$((n + 1))
		done <"$transcript"
		printf '</svg>\n'
	} >"$svg_out"
	echo "wrote $svg_out" >&2
fi
