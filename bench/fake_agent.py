#!/usr/bin/env python3
"""A stand-in for `claude -p --output-format stream-json --verbose`, for free dry runs of
bench/ab.py.

FAKE_AGENT_MODE picks what it does (default `solve`):
  solve    apply the task's reference solution (AB_FAKE_SOLUTION, or the AB_FAKE_SOLVE
           command) and succeed; a solution's answer.md becomes the final message
  noop     change nothing and succeed, so the hidden check fails
  crash    exit 1 with no output
  garbage  print text that isn't JSON
  error    print an error result and exit 1
  hang     sleep until the harness's timeout kills it
  ratelimit-once  report a rate limit on the first call, then solve (state in FAKE_STATE)
FAKE_COST is the cost it reports per run (default 0.01; `none` reports no cost).
FAKE_TOOLS lists the tool calls it makes, as `Name:count,…` (default `Read:2,Bash:1,Edit:1`).
A Bash call runs FAKE_BASH (default `ls -la`) through the folder's PreToolUse hooks, the
way Claude Code would: in a Lagom arm the hook rewrites it to `lagom run`, which compresses
the output and records it.
AB_FAKE_ARGV_LOG, if set, gets one JSON line per call: its arguments and the sign-in
variables it saw. In a folder whose .claude/settings.json
has Lagom's hooks, it runs the SessionStart hook the way Claude Code would, so the
harness sees whether the style was injected.
"""
import json
import os
import shutil
import subprocess
import sys
import time


def run_session_start_hooks(cwd):
    try:
        with open(os.path.join(cwd, ".claude", "settings.json")) as f:
            groups = json.load(f).get("hooks", {}).get("SessionStart", [])
    except (OSError, ValueError):
        return
    payload = json.dumps({"hook_event_name": "SessionStart", "source": "startup", "cwd": cwd,
                          "session_id": f"fake-{os.getpid()}"})
    for group in groups:
        for hook in group.get("hooks", []):
            subprocess.run(hook["command"], shell=True, input=payload, text=True, capture_output=True, timeout=10)


def hooks(cwd, event):
    try:
        with open(os.path.join(cwd, ".claude", "settings.json")) as f:
            groups = json.load(f).get("hooks", {}).get(event, [])
    except (OSError, ValueError):
        return []
    return [h["command"] for g in groups for h in g.get("hooks", [])]


def run_bash(cwd, command):
    """Run a Bash call as Claude Code does: PreToolUse hooks may replace the command."""
    payload = json.dumps({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd,
                          "permission_mode": "bypassPermissions", "session_id": f"fake-{os.getpid()}",
                          "tool_input": {"command": command}})
    for hook in hooks(cwd, "PreToolUse"):
        out = subprocess.run(hook, shell=True, input=payload, text=True, capture_output=True, timeout=10).stdout
        try:
            updated = json.loads(out)["hookSpecificOutput"]["updatedInput"]["command"]
        except (ValueError, KeyError, TypeError):
            continue
        command = updated
    p = subprocess.run(command, shell=True, cwd=cwd, text=True, capture_output=True, timeout=60)
    return p.stdout + p.stderr


def emit(event):
    print(json.dumps(event), flush=True)


def main(argv):
    log = os.environ.get("AB_FAKE_ARGV_LOG")
    if log:
        seen = {k: os.environ.get(k) for k in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL")}
        with open(log, "a") as f:
            f.write(json.dumps({"argv": argv, "env": seen}) + "\n")
    mode = os.environ.get("FAKE_AGENT_MODE", "solve")
    cost = None if os.environ.get("FAKE_COST") == "none" else float(os.environ.get("FAKE_COST", "0.01"))
    if mode == "ratelimit-once" and "Reply with the single word OK." in argv:
        mode = "solve"   # the warm-up: rate-limit the first real run instead
    if mode == "ratelimit-once":
        marker = os.path.join(os.environ["FAKE_STATE"], "rate-limited")
        if not os.path.exists(marker):
            open(marker, "w").close()
            print(json.dumps({"type": "result", "subtype": "error_during_execution", "is_error": True,
                              "result": "API Error: 429 rate limit exceeded", "total_cost_usd": 0}))
            return 1
        mode = "solve"
    cwd = os.getcwd()
    run_session_start_hooks(cwd)
    if mode == "hang":
        time.sleep(3600)
    if mode == "crash":
        print("fake agent: crashed", file=sys.stderr)
        return 1
    if mode == "garbage":
        print("this is not JSON")
        return 0
    solution = os.environ.get("AB_FAKE_SOLUTION")
    if mode == "solve" and solution and os.path.isdir(solution):
        shutil.copytree(solution, cwd, dirs_exist_ok=True)
    solve = json.loads(os.environ.get("AB_FAKE_SOLVE") or "[]")
    if mode == "solve" and solve:
        subprocess.run(solve, cwd=cwd, capture_output=True)
    error = mode == "error"
    model = "fake-model"
    emit({"type": "system", "subtype": "init", "model": model, "cwd": cwd})
    for spec in (os.environ.get("FAKE_TOOLS") or "Read:2,Bash:1,Edit:1").split(","):
        name, _, count = spec.partition(":")
        for _ in range(int(count or 1)):
            output = run_bash(cwd, os.environ.get("FAKE_BASH", "ls -la")) if name == "Bash" else "x" * 400
            emit({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": name, "input": {}}]}})
            emit({"type": "user", "message": {"content": [{"type": "tool_result", "content": output}]}})
    answer = os.path.join(solution or "", "answer.md")
    result = open(answer).read() if mode == "solve" and solution and os.path.isfile(answer) else "done"
    print(json.dumps({
        "type": "result", "subtype": "error_during_execution" if error else "success", "is_error": error,
        "num_turns": 3, "duration_ms": 1200, "duration_api_ms": 800, "total_cost_usd": cost, "result": result,
        "usage": {"input_tokens": 10, "output_tokens": 20},
        "modelUsage": {model: {"inputTokens": 100, "outputTokens": 200, "cacheReadInputTokens": 3000,
                               "cacheCreationInputTokens": 400, "costUSD": cost}},
    }))
    return 1 if error else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
