#!/usr/bin/env python3
"""Work out which crates a release still has to publish, and refuse the two
releases that should never happen.

A publish cannot be undone, so the job that does it should be hard to point at
the wrong thing. Two cases are treated as failures rather than as something to
skip quietly:

  * the version being released is older than one already on crates.io, which
    means an old release run is being replayed and would put a superseded
    version back on top;
  * every crate is already at this version, which means a finished release is
    being run again. Passing silently would hide that, so it fails.

A partly finished release, where one crate published and the next did not, is
not a failure: the missing crate is published and the finished one is skipped.

Usage: publish-plan.py CRATE [CRATE ...]
"""

import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

USER_AGENT = "gdmerge-release-workflow"


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def published_versions(crate):
    """Every version of `crate` on crates.io, or an empty list if it is new."""
    try:
        data = fetch(f"https://crates.io/api/v1/crates/{crate}")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return []
        raise
    return [v["num"] for v in data["versions"]]


def order(version):
    """Sort key matching how cargo orders release numbers.

    Enough for the versions this project publishes: numeric components compare
    as numbers, and a pre-release sorts below the same version without one.
    """
    core, _, pre = version.partition("-")
    numbers = tuple(int(part) if part.isdigit() else 0 for part in core.split("."))
    return (numbers, 0 if pre else 1, pre)


def workspace_version():
    """The version every crate in the workspace shares.

    They are meant to move together, so disagreement is a packaging mistake and
    is reported rather than guessed at.
    """
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    versions = {package["name"]: package["version"] for package in metadata["packages"]}
    distinct = set(versions.values())
    if len(distinct) != 1:
        sys.exit(f"error: workspace crates disagree on their version: {versions}")
    return distinct.pop()


def check_tag_matches(version):
    """A tag that names a different version than Cargo.toml means the wrong
    commit was tagged, which is worth catching before anything is uploaded."""
    ref = os.environ.get("GITHUB_REF_NAME", "")
    if not ref.startswith("v"):
        return None
    tagged = ref[1:]
    if tagged != version:
        return f"tag {ref} does not match the workspace version {version}"
    return None


def main(argv):
    crates = argv[1:]
    if not crates:
        sys.exit(f"usage: {argv[0]} CRATE [CRATE ...]")

    version = workspace_version()
    print(f"releasing version {version}")

    problems = []
    if mismatch := check_tag_matches(version):
        problems.append(mismatch)

    plan = {}
    for crate in crates:
        released = published_versions(crate)
        # Order matters: an old version that was itself published is a replay of
        # an old release, and saying so is more use than "already published".
        newer = [v for v in released if order(v) > order(version)]
        if newer:
            highest = max(newer, key=order)
            problems.append(
                f"{crate} {version} is older than {highest}, which is already on crates.io"
            )
            plan[crate] = False
            continue
        if version in released:
            print(f"  {crate} {version} is already on crates.io")
            plan[crate] = False
            continue
        print(f"  {crate} {version} needs publishing")
        plan[crate] = True

    if problems:
        sys.stdout.flush()
        for problem in problems:
            print(f"error: {problem}", file=sys.stderr)
        print("refusing to publish; nothing was uploaded", file=sys.stderr)
        return 1

    if not any(plan.values()):
        sys.stdout.flush()
        print(
            f"error: every crate is already at {version}; this release has nothing left to do",
            file=sys.stderr,
        )
        print(
            "re-running a finished release is almost always a mistake, so it fails "
            "rather than passing silently",
            file=sys.stderr,
        )
        return 1

    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"version={version}\n")
            for crate, needed in plan.items():
                handle.write(f"{crate}={'true' if needed else 'false'}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
