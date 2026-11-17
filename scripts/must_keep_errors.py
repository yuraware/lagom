"""Add every error line of a failing case to its must_keep (BEN-19).

    python3 -I scripts/must_keep_errors.py DIR...        # fill, then report what changed
    python3 -I scripts/must_keep_errors.py --check DIR...  # exit 1 if any line is missing

A case is a folder with input.txt and meta.json. An error line is what Lagom's
`is_error_line` matches (src/compress.rs), after colors are removed; a JSON Lines record
counts by the text in its "Output", escaped as in JSON. A must_keep entry stands for a line when it is the
whole line or at least 20 characters of it. `cargo test` checks the same rule
(bench::tests::a_failing_case_lists_every_error_line_in_must_keep).
"""

import json
import os
import re
import sys

MARKERS = ("error:", "error[", "fatal:", "panicked at", "traceback (most recent call last)",
           "err!", "exception in thread", "segmentation fault")
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b\n]*(?:\x07|\x1b\\)?|\x1b[@-_]")


def is_error_line(line):
    lower = line.lower()
    if any(m in lower for m in MARKERS):
        return True
    return any(w in ("FAILED", "FAIL") for w in re.split(r"[^A-Za-z0-9]", line))


def error_lines(text):
    found = []
    for line in ANSI.sub("", text).split("\n"):
        line = line[:-1] if line.endswith("\r") else line
        try:
            record = json.loads(line)
        except ValueError:
            record = None
        if isinstance(record, dict):
            # As written in JSON (`FAIL\\tpkg`): the JSON compressor keeps records as JSON.
            output = record.get("Output") if isinstance(record.get("Output"), str) else ""
            line = json.dumps(output.strip(), ensure_ascii=False)[1:-1]
        line = line.strip()
        if is_error_line(line) and line not in found:
            found.append(line)
    return sorted(found)


def covers(entry, line):
    return entry == line or (len(entry) >= 20 and entry in line)


def cases(root):
    for dirpath, _, files in sorted(os.walk(root)):
        if "meta.json" in files and "input.txt" in files:
            yield dirpath


def main(argv):
    check = "--check" in argv
    dirs = [a for a in argv if a != "--check"]
    missing_total = 0
    for root in dirs:
        for case in cases(root):
            meta_path = os.path.join(case, "meta.json")
            with open(meta_path) as f:
                meta = json.load(f)
            if meta.get("exit_code", 0) == 0:
                continue
            with open(os.path.join(case, "input.txt"), errors="replace") as f:
                text = f.read()
            keep = meta.setdefault("must_keep", [])
            missing = [l for l in error_lines(text) if not any(covers(k, l) for k in keep)]
            if not missing:
                continue
            missing_total += len(missing)
            if check:
                for line in missing:
                    print(f"{case}: error line not in must_keep: {line!r}")
            else:
                keep.extend(missing)
                with open(meta_path, "w") as f:
                    f.write(json.dumps(meta, ensure_ascii=False) + "\n")
                print(f"{case}: added {len(missing)} error lines to must_keep")
    return 1 if check and missing_total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
