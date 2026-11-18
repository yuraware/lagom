"""Checks a prose answer against a task's facts (BEN-10).

    python3 -I check_answer.py FACTS.json [ANSWER.md]     # default answer.md

facts.json: {"all": [regex, ...], "none": [regex, ...]}. Every `all` pattern must match the
answer and no `none` pattern may: case-insensitive, with ^ and $ matching at each line.
"""
import json
import re
import sys

with open(sys.argv[1]) as f:
    facts = json.load(f)
try:
    with open(sys.argv[2] if len(sys.argv) > 2 else "answer.md", errors="replace") as f:
        answer = f.read()
except OSError:
    sys.exit("no answer.md")
flags = re.IGNORECASE | re.MULTILINE
missing = [p for p in facts.get("all", []) if not re.search(p, answer, flags)]
present = [p for p in facts.get("none", []) if re.search(p, answer, flags)]
for p in missing:
    print(f"missing: {p}")
for p in present:
    print(f"must not appear: {p}")
print(f"{len(facts.get('all', [])) - len(missing)} of {len(facts.get('all', []))} facts, {len(present)} forbidden")
sys.exit(1 if missing or present else 0)
