#!/usr/bin/env python3
"""A stand-in for `claude -p --output-format json`, for free dry runs of bench/ab.py.

FAKE_AGENT_MODE picks what it does (default `solve`):
  solve    copy the task's reference solution in (AB_FAKE_SOLUTION) and succeed
  noop     change nothing and succeed, so the hidden check fails
  crash    exit 1 with no output
  garbage  print text that isn't JSON
  error    print an error result and exit 1
  hang     sleep until the harness's timeout kills it
FAKE_COST is the cost it reports per run (default 0.01). AB_FAKE_ARGV_LOG, if set, gets
one JSON line with the arguments of each call. In a folder whose .claude/settings.json
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


def main(argv):
    log = os.environ.get("AB_FAKE_ARGV_LOG")
    if log:
        with open(log, "a") as f:
            f.write(json.dumps(argv) + "\n")
    mode = os.environ.get("FAKE_AGENT_MODE", "solve")
    cost = float(os.environ.get("FAKE_COST", "0.01"))
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
    error = mode == "error"
    model = "fake-model"
    print(json.dumps({
        "type": "result", "subtype": "error_during_execution" if error else "success", "is_error": error,
        "num_turns": 3, "duration_ms": 1200, "total_cost_usd": cost, "result": "done",
        "usage": {"input_tokens": 10, "output_tokens": 20},
        "modelUsage": {model: {"inputTokens": 100, "outputTokens": 200, "cacheReadInputTokens": 3000,
                               "cacheCreationInputTokens": 400, "costUSD": cost}},
    }))
    return 1 if error else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
