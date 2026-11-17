#!/usr/bin/env python3
"""Fail when a change edits what a benchmark case must keep (BEN-19).

    python3 -I scripts/bench_meta_check.py [--base origin/main]

A case's meta.json says what compression must keep (`must_keep`) and which loss is a
known gap (`known_gap`). Rewording or dropping either makes the fidelity gate easier to
pass, as CMP-06 once did, so any change to them in an existing case, and any removed
case, needs the `bench-meta-ok` label on the pull request (CI sets
LAGOM_BENCH_META_OK=1). New cases need no label. Compares against `main`, not the
pull request's base, so a stack of pull requests can't move the baseline.
Standard library only.
"""
import argparse
import json
import subprocess
import sys

CORPORA = ("bench/corpus/", "tests/fixtures/compress/")
FIELDS = ("must_keep", "known_gap")


def git(root, *args):
    return subprocess.run(["git", "-C", root, *args], check=True, capture_output=True, text=True).stdout


def meta_at(root, rev, path):
    """The parsed meta.json at `rev` (None if absent). `rev` None means the working tree."""
    try:
        if rev is None:
            with open(f"{root}/{path}") as f:
                text = f.read()
        else:
            text = git(root, "show", f"{rev}:{path}")
    except (OSError, subprocess.CalledProcessError):
        return None
    try:
        return json.loads(text)
    except ValueError:
        return {"unparsable": text}


def changes(root, base):
    """(case, what changed) for every existing case whose must_keep or known_gap changed."""
    merge_base = git(root, "merge-base", base, "HEAD").strip()
    paths = git(root, "diff", "--name-only", merge_base, "--", *CORPORA).split()
    found = []
    for path in sorted(p for p in paths if p.endswith("/meta.json")):
        before = meta_at(root, merge_base, path)
        if before is None:
            continue  # a new case
        after = meta_at(root, None, path)
        case = path[: -len("/meta.json")]
        if after is None:
            found.append((case, "case removed"))
            continue
        for field in FIELDS:
            if before.get(field) != after.get(field):
                found.append((case, f"{field}: {json.dumps(before.get(field))} -> {json.dumps(after.get(field))}"))
    return found


def main(argv, environ):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--base", default="origin/main")
    p.add_argument("--root", default=".")
    args = p.parse_args(argv)
    found = changes(args.root, args.base)
    for case, what in found:
        print(f"{case}: {what}")
    if not found:
        print("bench meta check ok: no must_keep or known_gap changed")
        return 0
    if environ.get("LAGOM_BENCH_META_OK") == "1":
        print(f"{len(found)} change(s) allowed by the bench-meta-ok label")
        return 0
    print(
        f"{len(found)} change(s) to must_keep or known_gap. These decide what the fidelity gate "
        "checks, so a maintainer must agree: add the bench-meta-ok label and say why in the "
        "pull request."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:], __import__("os").environ))
