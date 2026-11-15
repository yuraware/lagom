#!/usr/bin/env python3
"""Merge changelog.d/ fragments into CHANGELOG.md, or check that a change has one.

    python3 -I scripts/release_notes.py 0.2.0 [--date 2026-12-01] [--dry-run]
    python3 -I scripts/release_notes.py --check [--base origin/main]

Standard library only.
"""
import argparse
import datetime
import os
import re
import subprocess
import sys

HEADINGS = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]


class FragmentError(Exception):
    pass


def parse_fragment(name, text):
    """Return {heading: [bullet, ...]} from one fragment."""
    sections, heading = {}, None
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        m = re.match(r"^###\s+(.+?)\s*$", line)
        if m:
            heading = m.group(1)
            if heading not in HEADINGS:
                raise FragmentError(f"{name}:{n}: unknown heading {heading!r} (use {', '.join(HEADINGS)})")
            sections.setdefault(heading, [])
        elif line.startswith("- "):
            if heading is None:
                raise FragmentError(f"{name}:{n}: bullet before any ### heading")
            sections[heading].append(line)
        elif line.startswith((" ", "\t")) and heading and sections[heading]:
            sections[heading][-1] += "\n" + line  # continuation of the previous bullet
        else:
            raise FragmentError(f"{name}:{n}: expected '### Heading' or '- bullet', got {line!r}")
    if not any(sections.values()):
        raise FragmentError(f"{name}: no bullets")
    return sections


def read_fragments(directory):
    names = sorted(f for f in os.listdir(directory) if f.endswith(".md") and f != "README.md")
    merged = {h: [] for h in HEADINGS}
    for name in names:
        with open(os.path.join(directory, name), encoding="utf-8") as fh:
            for heading, bullets in parse_fragment(name, fh.read()).items():
                merged[heading].extend(bullets)
    return names, merged


def render_section(version, date, merged):
    out = [f"## [{version}] - {date}", ""]
    for heading in HEADINGS:
        if merged[heading]:
            out += [f"### {heading}", *merged[heading], ""]
    return "\n".join(out)


def insert_section(changelog, section, version):
    if re.search(rf"^## \[{re.escape(version)}\]", changelog, re.M):
        raise FragmentError(f"CHANGELOG.md already has a [{version}] section")
    m = re.search(r"^## \[", changelog, re.M)
    if m:
        return changelog[: m.start()] + section + "\n" + changelog[m.start():]
    return changelog.rstrip("\n") + "\n\n" + section


def release(root, version, date, dry_run):
    frag_dir = os.path.join(root, "changelog.d")
    names, merged = read_fragments(frag_dir)
    if not names:
        raise FragmentError("no fragments in changelog.d/")
    section = render_section(version, date, merged)
    path = os.path.join(root, "CHANGELOG.md")
    with open(path, encoding="utf-8") as fh:
        updated = insert_section(fh.read(), section, version)
    if dry_run:
        print(section)
        return
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(updated)
    for name in names:
        os.remove(os.path.join(frag_dir, name))
    print(f"CHANGELOG.md: added [{version}] from {len(names)} fragment(s)")


# What users get: the code, and the styles and skills built into the binary.
USER_VISIBLE = ("src/", "styles/", "skills/")


def changed_files(root, base, diff_filter=None):
    cmd = ["git", "diff", "--name-only"]
    if diff_filter:
        cmd.append(f"--diff-filter={diff_filter}")
    cmd.append(f"{base}...HEAD")
    return subprocess.run(cmd, cwd=root, capture_output=True, text=True, check=True).stdout.split()


def check(root, base):
    """Fail when user-visible files changed but no fragment was added (label `no-changelog`
    skips this). Only a newly added `changelog.d/*.md` counts: editing or deleting an
    existing fragment doesn't describe this change."""
    if os.environ.get("LAGOM_NO_CHANGELOG") == "1":
        print("changelog check skipped (no-changelog label)")
        return 0
    names, _ = read_fragments(os.path.join(root, "changelog.d"))  # also validates every fragment
    code_changed = any(p.startswith(USER_VISIBLE) for p in changed_files(root, base))
    fragment_added = any(
        p.startswith("changelog.d/") and p.endswith(".md") and p != "changelog.d/README.md"
        for p in changed_files(root, base, "A")
    )
    if code_changed and not fragment_added:
        print(f"{', '.join(USER_VISIBLE)} changed but no changelog.d/ fragment was added (see changelog.d/README.md)")
        return 1
    print(f"changelog check ok ({len(names)} pending fragment(s))")
    return 0


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("version", nargs="?")
    p.add_argument("--date", default=datetime.date.today().isoformat())
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--check", action="store_true")
    p.add_argument("--base", default="origin/main")
    p.add_argument("--root", default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    a = p.parse_args(argv)
    try:
        if a.check:
            return check(a.root, a.base)
        if not a.version or not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", a.version):
            p.error("give a version like 0.2.0")
        release(a.root, a.version, a.date, a.dry_run)
        return 0
    except (FragmentError, subprocess.CalledProcessError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
