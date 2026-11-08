#!/usr/bin/env bash
# Latency of the hot paths, measured with hyperfine (https://github.com/sharkdp/hyperfine):
#   hook session-start, hook pre-tool, `lagom run -- true` vs `true`, compress 1 MB.
#
#   bench/latency.sh [out.json]      default out: target/bench/latency.json
#
# Runs against a throwaway HOME, so your agent config and Lagom stats are untouched.
# Budgets (checked in CI by QA-08): hooks < 10 ms, run overhead < 5 ms, 1 MB < 20 ms.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/target/bench/latency.json}
command -v hyperfine >/dev/null || {
  echo "bench: hyperfine not found. Install it: brew install hyperfine | apt install hyperfine | cargo install hyperfine" >&2
  exit 2
}
cargo build -q --release --manifest-path "$ROOT/Cargo.toml"
BIN="$ROOT/target/release/lagom"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/lagom-bench.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
export HOME="$WORK/home" LAGOM_HOME="$WORK/lagom-home"
unset CLAUDE_CONFIG_DIR CODEX_HOME XDG_CONFIG_HOME LAGOM_STYLES LAGOM_DISABLE LAGOM_REWRITE
mkdir -p "$HOME/repo"

printf '{"hook_event_name":"SessionStart","source":"startup","cwd":"%s/repo"}' "$HOME" > "$WORK/session.json"
printf '{"hook_event_name":"PreToolUse","tool_name":"Bash","permission_mode":"bypassPermissions","cwd":"%s/repo","tool_input":{"command":"git log --stat"}}' "$HOME" > "$WORK/pretool.json"
# About 1 MB of real outputs: the golden compressor fixtures, repeated.
: > "$WORK/1mb.txt"
while [ "$(wc -c < "$WORK/1mb.txt")" -lt 1048576 ]; do
  cat "$ROOT"/tests/fixtures/compress/*/input.txt >> "$WORK/1mb.txt"
done
"$BIN" --version > /dev/null   # first run of a new binary can be slow on macOS (code scan)

run() { # name input-file command...
  local name=$1 input=$2; shift 2
  hyperfine -N --warmup 5 --runs 50 --style basic --input "$input" \
    --export-json "$WORK/$name.json" --command-name "$name" -- "$*" > /dev/null
}
run hook-session-start "$WORK/session.json" "$BIN" hook claude session-start
run hook-pre-tool      "$WORK/pretool.json" "$BIN" hook claude pre-tool
run run-true           /dev/null            "$BIN" run -- true
run true               /dev/null            true
run compress-1mb       "$WORK/1mb.txt"      "$BIN" compress

mkdir -p "$(dirname "$OUT")"
python3 -I - "$WORK" "$OUT" <<'PY'
import json, sys, platform
work, out = sys.argv[1], sys.argv[2]
names = ["hook-session-start", "hook-pre-tool", "run-true", "true", "compress-1mb"]
rows = {}
for n in names:
    r = json.load(open(f"{work}/{n}.json"))["results"][0]
    # hyperfine 2.x nests timings under summary.time_wall_clock; 1.x has them at the top.
    s = r.get("summary", {}).get("time_wall_clock", r)
    rows[n] = {"median_ms": round(s["median"] * 1000, 3), "mean_ms": round(s["mean"] * 1000, 3),
               "stddev_ms": round((s.get("stddev") or 0) * 1000, 3), "max_ms": round(s["max"] * 1000, 3)}
    mem = r.get("summary", {}).get("memory_peak_resident")
    if mem:
        rows[n]["peak_rss_mb"] = round(mem["max"] / 1048576, 2)
rows["run-overhead"] = {"median_ms": round(rows["run-true"]["median_ms"] - rows["true"]["median_ms"], 3)}
json.dump({"platform": f"{platform.system()} {platform.machine()}", "results": rows}, open(out, "w"), indent=2)
budget = {"hook-session-start": 10, "hook-pre-tool": 10, "run-overhead": 5, "compress-1mb": 20}
print(f"{'measure':<20} {'median':>9}  budget")
for n, r in rows.items():
    b = budget.get(n)
    mark = "" if b is None else ("ok" if r["median_ms"] < b else "OVER")
    print(f"{n:<20} {r['median_ms']:>7.2f}ms  {'' if b is None else f'<{b}ms {mark}'}")
print(f"written: {out}")
PY
