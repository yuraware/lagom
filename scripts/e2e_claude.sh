#!/usr/bin/env bash
# Live end-to-end check of Lagom inside Claude Code. Costs one short Haiku session.
#
# Never touches user-level settings: the hooks live in a throwaway repo's
# .claude/settings.local.json, and `--setting-sources project,local` skips
# ~/.claude/settings.json (and any other tool's hooks installed there).
# Lagom's data goes to a temporary LAGOM_HOME.
#
# Passes when the session-start style injection fired, a `git show` was rewritten
# through `lagom run` and compressed, and the agent still answered correctly (300 files).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
command -v claude >/dev/null || { echo "e2e: claude CLI not found on PATH" >&2; exit 2; }
cargo build -q --manifest-path "$ROOT/Cargo.toml"
BIN="$ROOT/target/debug/lagom"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/lagom-e2e.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
export LAGOM_HOME="$WORK/lagom-home"
# The default styles, set explicitly: your own ~/.config/lagom/config.json (for example
# styles turned off) must not change the result.
export LAGOM_STYLES="terse,lean-code"
unset LAGOM_DISABLE

REPO="$WORK/repo"
mkdir -p "$REPO/.claude"
cd "$REPO"
git init -q
git config user.email e2e@example.com
git config user.name e2e
for i in $(seq 1 300); do printf 'line %s\n' "$i" > "file_$i.txt"; done
git add -A
git commit -q -m "add 300 files"

cat > .claude/settings.local.json <<JSON
{
  "permissions": { "allow": ["Bash(git show:*)"] },
  "hooks": {
    "SessionStart": [{ "matcher": "startup|resume|clear|compact",
      "hooks": [{ "type": "command", "command": "'$BIN' hook claude session-start", "timeout": 10 }] }],
    "PreToolUse": [{ "matcher": "Bash",
      "hooks": [{ "type": "command", "command": "'$BIN' hook claude pre-tool", "timeout": 10 }] }]
  }
}
JSON

echo "e2e: running Claude Code (haiku) in $REPO"
claude -p --setting-sources project,local --model haiku \
  "Use the Bash tool to run exactly: git show --stat HEAD. Then reply with only the number of files changed." \
  > "$WORK/answer.txt" 2> "$WORK/claude.err" || { cat "$WORK/claude.err" >&2; exit 1; }
echo "e2e: answer: $(tr -d '\n' < "$WORK/answer.txt")"

python3 -I - "$LAGOM_HOME/stats.jsonl" "$WORK/answer.txt" <<'PY'
import json, re, sys
try:
    events = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
except FileNotFoundError:
    events = []
answer = open(sys.argv[2]).read()
inject = [e for e in events if e.get("kind") == "inject"]
runs = [e for e in events if e.get("kind") == "run" and e.get("cmd") == "git"]
compressed = [e for e in runs if e.get("in", 0) > e.get("out", 0)]
print(f"e2e: injections={len(inject)} git runs={len(runs)} compressed={len(compressed)}")
for e in compressed:
    print(f"e2e: git output {e['in']} -> {e['out']} bytes")
# Compression must not cost correctness: the commit changed exactly 300 files.
correct = re.search(r"\b300\b", answer) is not None
print(f"e2e: answer correct={correct}")
ok = inject and compressed and correct
print("e2e: PASS" if ok else "e2e: FAIL")
sys.exit(0 if ok else 1)
PY
