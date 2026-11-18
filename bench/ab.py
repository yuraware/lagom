#!/usr/bin/env python3
"""A/B harness for coding agents (BEN-06): the same tasks under each arm, interleaved,
each run in a fresh folder and judged by hidden checks. Standard library only.

    python3 -I bench/ab.py --selftest
    python3 -I bench/ab.py --agent fake --arms baseline,terse-control,lagom:terse --repeats 2
    python3 -I bench/ab.py --agent claude --arms baseline,terse-control,lagom:terse \\
        --repeats 3 --budget-usd 20 [--model sonnet] [--task smoke/py-off-by-one,smoke/py-add-flag]

Arms: `baseline`; `terse-control`, with only the instruction "Answer concisely."; and
`lagom:<style>`, with Lagom's hooks and that style installed in the run's folder.

A task is a folder under bench/tasks/ (grouped in suites: smoke/, rust/, python/,
output-heavy/) with task.json (`prompt`, `check` as an argument list, `bash`: whether the
agent may run commands, `answer`: whether the agent's final message is the result, saved
as answer.md for the check, and optional `setup` and `solve` commands), fixture/ (what the
agent starts with, plus whatever `setup` generates there), hidden/ (checks copied in only
after the agent stops) and solution/ or `solve` (a reference the checks must pass). `--selftest` runs every task's references: the
fixture alone must fail its checks and fixture plus solution must pass. A paid run
refuses to start if it fails.

Every run happens outside any git repository, after one throwaway call per model so no
arm pays the cold cache write. Claude Code runs with --setting-sources project
--strict-mcp-config --no-session-persistence and permissions bypassed, never --bare
(it skips hooks). A task that allows commands runs them in Claude Code's own sandbox:
writes stay in the run's folder and no network leaves it. Codex runs with
--sandbox workspace-write. Tokens are read from `modelUsage`, which counts subagents.

Auth modes (BEN-21): `--auth api` uses ANTHROPIC_API_KEY (OPENAI_API_KEY for Codex) and
`--budget-usd`; `--auth login` uses the agent's own subscription login, so costs are
list-price estimates, and on a rate limit the harness waits and retries; `--auth local`
runs Claude Code against a local model through Ollama's Anthropic-compatible API (Codex
with --oss), costs nothing, and checks that hooks fire, the style is injected and the
JSON parses. Local results are never published. `--resume` skips runs already in the
results file, so an interrupted run continues where it stopped.

Results: one JSON line per run in bench/results/<date>/runs.jsonl (bench/results/<date>-local
for local runs), each with its auth mode.
"""
import argparse
import datetime
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
CONTROL = "Answer concisely."
ISOLATION = ["--setting-sources", "project", "--strict-mcp-config", "--no-session-persistence"]
WARMUP_PROMPT = "Reply with the single word OK."
CHECK_TIMEOUT = 300
LOCAL_URL = "http://localhost:11434"   # Ollama; no /v1: that path is its OpenAI layer
RATE_LIMIT_MARKERS = ("rate limit", "rate_limit", "usage limit", "429", "overloaded")


class UsageError(Exception):
    pass


# ---------- tasks ----------

def load_tasks(root, only=None):
    """Every folder under `root` with a task.json, named by its path (`rust/off-by-one`)."""
    root = os.path.abspath(root)   # commands run in the run's folder: `{task}` must be absolute
    tasks = []
    for d, dirs, files in sorted(os.walk(root)):
        dirs[:] = sorted(x for x in dirs if x not in ("fixture", "hidden", "solution"))
        if "task.json" not in files:
            continue
        name = os.path.relpath(d, root).replace(os.sep, "/")
        meta_path = os.path.join(d, "task.json")
        if only and name not in only:
            continue
        with open(meta_path) as f:
            meta = json.load(f)
        if not isinstance(meta.get("prompt"), str) or not isinstance(meta.get("check"), list):
            raise UsageError(f"{meta_path}: needs a `prompt` string and a `check` argument list")

        # In commands, `{bench}` is this folder (runners shared by a suite) and `{task}` the
        # task's folder (scripts the agent never sees).
        def cmd(key, d=d, meta=meta):
            return [a.replace("{bench}", HERE).replace("{task}", d) for a in meta[key]] if meta.get(key) else None

        tasks.append({"name": name, "dir": d, "prompt": meta["prompt"], "check": cmd("check"),
                      "setup": cmd("setup"), "solve": cmd("solve"), "bash": bool(meta.get("bash", False)),
                      "answer": bool(meta.get("answer", False))})
    missing = sorted(set(only or []) - {t["name"] for t in tasks})
    if missing:
        raise UsageError(f"no such task: {', '.join(missing)}")
    if not tasks:
        raise UsageError(f"no tasks in {root}")
    return tasks


def inside_git(path):
    return subprocess.run(["git", "-C", path, "rev-parse", "--is-inside-work-tree"],
                          capture_output=True).returncode == 0


def workspace(task, overlay=None):
    """A fresh folder outside any git repository with the task's fixture in `work/`, after
    the task's `setup` command (which writes large generated files) has run there."""
    base = tempfile.mkdtemp(prefix="lagom-ab-")
    if inside_git(base):
        shutil.rmtree(base)
        raise UsageError(f"{tempfile.gettempdir()} is inside a git repository: set TMPDIR elsewhere")
    work = os.path.join(base, "work")
    shutil.copytree(os.path.join(task["dir"], "fixture"), work)
    if task.get("setup"):
        p = subprocess.run(task["setup"], cwd=work, capture_output=True, text=True, timeout=CHECK_TIMEOUT)
        if p.returncode != 0:
            shutil.rmtree(base, ignore_errors=True)
            raise UsageError(f"{task['name']}: setup failed: {(p.stdout + p.stderr).strip()[-300:]}")
    if overlay:
        shutil.copytree(overlay, work, dirs_exist_ok=True)
    return base, work


def run_check(task, work):
    """Copy the hidden checks in and run them: (passed, last lines of their output)."""
    hidden = os.path.join(task["dir"], "hidden")
    if os.path.isdir(hidden):
        shutil.copytree(hidden, work, dirs_exist_ok=True)
    try:
        p = subprocess.run(task["check"], cwd=work, capture_output=True, text=True, timeout=CHECK_TIMEOUT)
    except subprocess.TimeoutExpired:
        return False, "check timed out"
    except OSError as e:
        return False, f"check could not run: {e}"
    return p.returncode == 0, "\n".join((p.stdout + p.stderr).strip().splitlines()[-5:])


def selftest(tasks):
    """Problems with the tasks' references: the fixture must fail, the solution pass."""
    problems = []
    for t in tasks:
        solution = os.path.join(t["dir"], "solution")
        if not os.path.isdir(solution) and not t.get("solve"):
            problems.append(f"{t['name']}: no solution/ folder or `solve` command")
            continue
        overlay = solution if os.path.isdir(solution) else None
        for label, use, want in (("fixture alone", None, False), ("solution", overlay, True)):
            try:
                base, work = workspace(t, use)
            except UsageError as e:
                problems.append(str(e))
                break
            try:
                if label == "solution" and t.get("solve"):
                    subprocess.run(t["solve"], cwd=work, capture_output=True, timeout=CHECK_TIMEOUT)
                passed, _ = run_check(t, work)
            finally:
                shutil.rmtree(base, ignore_errors=True)
            if passed != want:
                problems.append(f"{t['name']}: the {label} {'passes' if passed else 'fails'} the hidden check")
    return problems


# ---------- arms ----------

def parse_arms(text):
    arms = [a.strip() for a in text.split(",") if a.strip()]
    for a in arms:
        if a in ("baseline", "terse-control"):
            continue
        style = a.split(":", 1)[1] if a.startswith("lagom:") else ""
        if not style or not all(c.isalnum() or c in "-_" for c in style):
            raise UsageError(f"unknown arm {a!r}: use baseline, terse-control or lagom:<style>")
    if len(set(arms)) != len(arms) or not arms:
        raise UsageError("arms must be listed once each")
    return arms


def lagom_hooks(lagom_bin, agent):
    """Lagom's hook entries, as `lagom install` writes them (src/install.rs)."""
    def group(event, matcher=None):
        g = {"hooks": [{"type": "command", "timeout": 10,
                        "command": f"{shlex.quote(lagom_bin)} hook {agent} {event}"}]}
        if matcher:
            g = {"matcher": matcher, **g}
        return [g]
    hooks = {"SessionStart": group("session-start", "startup|resume|clear|compact|fork"),
             "PreToolUse": group("pre-tool", "Bash" if agent == "claude" else "Bash|bash")}
    if agent == "claude":
        hooks["SubagentStart"] = group("subagent-start")
    return hooks


def prepare(work, agent, arm, task, lagom_bin):
    """Write the run's project settings: Lagom's hooks and style, and the Bash sandbox."""
    hook_agent = "codex" if agent == "codex" else "claude"
    settings = {}
    if task["bash"] and hook_agent == "claude":
        settings["sandbox"] = {"enabled": True, "allowUnsandboxedCommands": False}
    if arm.startswith("lagom:"):
        os.makedirs(os.path.join(work, ".lagom"), exist_ok=True)
        with open(os.path.join(work, ".lagom", "config.json"), "w") as f:
            json.dump({"styles": [arm.split(":", 1)[1]]}, f)
        if hook_agent == "claude":
            settings["hooks"] = lagom_hooks(lagom_bin, "claude")
        else:
            os.makedirs(os.path.join(work, ".codex"), exist_ok=True)
            with open(os.path.join(work, ".codex", "hooks.json"), "w") as f:
                json.dump({"hooks": lagom_hooks(lagom_bin, "codex")}, f)
    if settings:
        os.makedirs(os.path.join(work, ".claude"), exist_ok=True)
        with open(os.path.join(work, ".claude", "settings.json"), "w") as f:
            json.dump(settings, f, indent=2)


# ---------- agents ----------

def command(agent, agent_bin, prompt, arm, model, bash, budget_left, auth="api"):
    if agent == "codex":
        cmd = [agent_bin, "exec", "--json", "--skip-git-repo-check", "--ephemeral", "--ignore-user-config",
               "--sandbox", "workspace-write"]
        if model:
            cmd += ["--model", model]
        if arm == "terse-control":
            cmd += ["-c", f"developer_instructions={json.dumps(CONTROL)}"]
        if arm.startswith("lagom:"):
            cmd += ["--dangerously-bypass-hook-trust"]
        if auth == "local":
            cmd += ["--oss", "--local-provider", "ollama"]
        return cmd + [prompt]
    cmd = [agent_bin, "-p", prompt, "--output-format", "json", *ISOLATION, "--permission-mode", "bypassPermissions"]
    if agent == "fake":
        cmd = [sys.executable, "-I", *cmd]
    if model:
        cmd += ["--model", model]
    if arm == "terse-control":
        cmd += ["--append-system-prompt", CONTROL]
    if not bash:
        cmd += ["--disallowedTools", "Bash"]
    if budget_left is not None:
        cmd += ["--max-budget-usd", f"{max(budget_left, 0.01):.2f}"]
    return cmd


def parse_claude(stdout):
    """The result of `claude -p --output-format json`, or None when it isn't one."""
    text = stdout.strip()
    for candidate in (text, text.splitlines()[-1] if text else ""):
        try:
            r = json.loads(candidate)
        except ValueError:
            continue
        if isinstance(r, dict) and r.get("type") == "result":
            usage = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
            for m in (r.get("modelUsage") or {}).values():
                usage["input"] += m.get("inputTokens", 0)
                usage["output"] += m.get("outputTokens", 0)
                usage["cache_read"] += m.get("cacheReadInputTokens", 0)
                usage["cache_write"] += m.get("cacheCreationInputTokens", 0)
            return {"cost_usd": r.get("total_cost_usd"), "turns": r.get("num_turns"),
                    "agent_ms": r.get("duration_ms"), "is_error": bool(r.get("is_error")),
                    "subtype": r.get("subtype"), "models": sorted((r.get("modelUsage") or {}).keys()),
                    "usage": usage, "result": r.get("result") if isinstance(r.get("result"), str) else ""}
    return None


def parse_codex(stdout):
    """`codex exec --json` events: tokens from each turn.completed. Codex reports no cost."""
    usage = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
    turns, error, seen, result = 0, False, False, ""
    for line in stdout.splitlines():
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if not isinstance(e, dict):
            continue
        seen = True
        if e.get("type") == "turn.completed":
            turns += 1
            u = e.get("usage") or {}
            usage["input"] += u.get("input_tokens", 0)
            usage["cache_read"] += u.get("cached_input_tokens", 0)
            usage["output"] += u.get("output_tokens", 0)
        elif e.get("type") in ("turn.failed", "error"):
            error = True
        elif e.get("type") == "item.completed" and (e.get("item") or {}).get("type") == "agent_message":
            result = e["item"].get("text") or result   # the last message is the answer
    if not seen:
        return None
    return {"cost_usd": None, "turns": turns, "agent_ms": None, "is_error": error, "subtype": None,
            "models": [], "usage": usage, "result": result}


def run_agent(cmd, cwd, env, timeout):
    """(exit code or None on timeout, stdout, stderr). The whole process group is killed on timeout."""
    p = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        out, err = p.communicate(timeout=timeout)
        return p.returncode, out, err
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, err = p.communicate()
        return None, out, err


def lagom_activity(lagom_home):
    """How often Lagom acted in a run: style injections and compressed commands."""
    counts = {"injections": 0, "compressed": 0}
    try:
        with open(os.path.join(lagom_home, "stats.jsonl")) as f:
            for line in f:
                kind = json.loads(line).get("kind")
                counts["injections"] += kind == "inject"
                counts["compressed"] += kind == "run"
    except (OSError, ValueError):
        pass
    return counts


def agent_env(base, arm, task_dir, agent, auth="api", local_url=LOCAL_URL, solve=None):
    env = dict(os.environ)
    if auth == "login":
        # Without a key or another endpoint, the agent signs in with its own login.
        for var in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "OPENAI_API_KEY", "CODEX_API_KEY"):
            env.pop(var, None)
    elif auth == "local" and agent != "codex":
        env.update(ANTHROPIC_BASE_URL=local_url, ANTHROPIC_AUTH_TOKEN="ollama", ANTHROPIC_API_KEY="")
    env["LAGOM_HOME"] = os.path.join(base, "lagom-home")       # this run's stats only
    env["XDG_CONFIG_HOME"] = os.path.join(base, "config")      # no user Lagom styles or config
    env.pop("LAGOM_STYLES", None)
    env.pop("LAGOM_REWRITE", None)
    if arm.startswith("lagom:"):
        env.pop("LAGOM_DISABLE", None)
    else:
        env["LAGOM_DISABLE"] = "1"   # a user-level Lagom hook, if one still loads, does nothing
    if agent == "fake":
        env["AB_FAKE_SOLUTION"] = os.path.join(task_dir, "solution") if task_dir else ""
        env["AB_FAKE_SOLVE"] = json.dumps(solve or [])
    return env


# ---------- the run ----------

def one_run(args, task, arm, repeat, budget_left):
    base, work = workspace(task)
    try:
        prepare(work, args.agent, arm, task, args.lagom_bin)
        env = agent_env(base, arm, task["dir"], args.agent, args.auth, args.local_url, task.get("solve"))
        cmd = command(args.agent, args.agent_bin, task["prompt"], arm, args.model, task["bash"], budget_left, args.auth)
        started = datetime.datetime.now(datetime.timezone.utc)
        code, out, err = run_agent(cmd, work, env, args.timeout)
        wall_ms = int((datetime.datetime.now(datetime.timezone.utc) - started).total_seconds() * 1000)
        parsed = (parse_codex if args.agent == "codex" else parse_claude)(out)
        if code is None:
            status = "timeout"
        elif (code != 0 or parsed is None or parsed["is_error"]) and rate_limited(out + err):
            status = "rate_limited"
        elif parsed is None:
            status = "bad_output"
        elif code != 0 or parsed["is_error"]:
            status = "agent_error"
        else:
            status = "ok"
        answer = (parsed or {}).pop("result", "") if parsed else ""
        if task.get("answer"):
            # Prose tasks are judged on the agent's final message.
            with open(os.path.join(work, "answer.md"), "w") as f:
                f.write(answer)
        passed, check_tail = run_check(task, work)
        row = {"kind": "run", "ts": started.isoformat(timespec="seconds"), "agent": args.agent,
               "auth": args.auth, "model": args.model, "task": task["name"], "arm": arm, "repeat": repeat,
               "status": status,
               "exit_code": code, "passed": passed, "wall_ms": wall_ms, **(parsed or {}),
               "lagom": lagom_activity(env["LAGOM_HOME"]) if arm.startswith("lagom:") else None,
               "answer_words": len(answer.split()) if task.get("answer") else None}
        if task.get("answer"):
            row["answer"] = answer
        row.pop("is_error", None)
        if status != "ok":
            row["stderr_tail"] = "\n".join(err.strip().splitlines()[-5:])
        if not passed:
            row["check_tail"] = check_tail
        return row
    finally:
        shutil.rmtree(base, ignore_errors=True)


def rate_limited(text):
    lower = text.lower()
    return any(m in lower for m in RATE_LIMIT_MARKERS)


def warmup(args, budget_left):
    """One throwaway call, so no arm pays the cold cache write."""
    base = tempfile.mkdtemp(prefix="lagom-ab-warmup-")
    try:
        cmd = command(args.agent, args.agent_bin, WARMUP_PROMPT, "baseline", args.model, False, budget_left, args.auth)
        env = agent_env(base, "baseline", None, args.agent, args.auth, args.local_url)
        wait = args.backoff
        for _ in range(args.max_retries + 1):
            code, out, err = run_agent(cmd, base, env, args.timeout)
            if code == 0 or not rate_limited(out + err):
                break
            print(f"ab: rate limited during the warm-up; waiting {wait:.0f} s", file=sys.stderr)
            time.sleep(wait)
            wait = min(wait * 2, 3600)
        parsed = (parse_codex if args.agent == "codex" else parse_claude)(out) or {}
        return {"kind": "warmup", "agent": args.agent, "auth": args.auth, "model": args.model, "exit_code": code,
                "cost_usd": parsed.get("cost_usd"), "models": parsed.get("models", [])}
    finally:
        shutil.rmtree(base, ignore_errors=True)


def schedule(tasks, arms, repeats):
    """(repeat, task, arm) cells, arms interleaved and their order rotated per task and repeat."""
    cells = []
    for r in range(repeats):
        for i, t in enumerate(tasks):
            k = (r + i) % len(arms)
            cells.extend((r, t, a) for a in arms[k:] + arms[:k])
    return cells


AUTH_NOTES = {
    "api": "API key: costs are Claude Code's estimates at list prices",
    "login": "subscription login: costs are list-price estimates, not what the plan bills",
    "local": "local model: for checking the harness only, never published",
    "fake": "fake agent: a dry run",
}


def smoke_problems(rows):
    """What a local smoke run must show: parsable output, and hooks that inject the style."""
    problems = [f"{r['task']} {r['arm']}: the agent's output didn't parse" for r in rows if r["status"] == "bad_output"]
    problems += [f"{r['task']} {r['arm']}: the style was never injected" for r in rows
                 if r["arm"].startswith("lagom:") and not (r.get("lagom") or {}).get("injections")]
    return problems


def summary(rows, arms, auth):
    lines = ["", f"Auth: {auth} ({AUTH_NOTES[auth]})",
             f"{'arm':<20} {'runs':>4} {'passed':>6} {'median cost':>12} {'median turns':>13}"]
    for arm in arms:
        rs = [r for r in rows if r["arm"] == arm]
        costs = sorted(r["cost_usd"] for r in rs if r.get("cost_usd") is not None)
        turns = sorted(r["turns"] for r in rs if r.get("turns") is not None)
        med = lambda v: v[len(v) // 2] if v else None
        cost = f"${med(costs):.4f}" if costs else "–"
        lines.append(f"{arm:<20} {len(rs):>4} {sum(r['passed'] for r in rs):>6} {cost:>12} {str(med(turns) or '–'):>13}")
    return "\n".join(lines)


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--agent", choices=["claude", "codex", "fake"])
    p.add_argument("--agent-bin", help="the agent's executable (default: claude, codex, or bench/fake_agent.py)")
    p.add_argument("--arms", default="baseline,terse-control,lagom:terse")
    p.add_argument("--tasks", default=os.path.join(HERE, "tasks"))
    p.add_argument("--task", help="only these tasks, comma-separated")
    p.add_argument("--repeats", type=int, default=1)
    p.add_argument("--budget-usd", type=float, help="stop starting runs at this spend (required for claude)")
    p.add_argument("--max-runs", type=int, help="stop after this many runs (required for codex: it reports no cost)")
    p.add_argument("--model")
    p.add_argument("--timeout", type=int, default=900, help="seconds per run (default 900)")
    p.add_argument("--auth", choices=["api", "login", "local"], help="how the agent signs in (default api; fake: none)")
    p.add_argument("--local-url", default=LOCAL_URL, help=f"the local model server for --auth local (default {LOCAL_URL})")
    p.add_argument("--backoff", type=float, default=60, help="seconds to wait after a rate limit, doubling each time")
    p.add_argument("--max-retries", type=int, default=6, help="rate limits in a row before giving up")
    p.add_argument("--resume", action="store_true", help="skip runs already in the results file")
    p.add_argument("--lagom-bin", default=shutil.which("lagom") or "lagom")
    p.add_argument("--out", help="results folder (default bench/results/<date>, with -local for local runs)")
    p.add_argument("--selftest", action="store_true", help="run every task's references and stop")
    args = p.parse_args(argv)
    try:
        tasks = load_tasks(args.tasks, args.task.split(",") if args.task else None)
        if args.selftest:
            problems = selftest(tasks)
            print("\n".join(problems) or f"selftest ok: {len(tasks)} tasks")
            return 1 if problems else 0
        if not args.agent:
            raise UsageError("--agent is required (or --selftest)")
        arms = parse_arms(args.arms)
        if args.repeats < 1:
            raise UsageError("--repeats must be at least 1")
        args.auth = args.auth or ("fake" if args.agent == "fake" else "api")
        if args.agent == "claude" and args.auth != "local" and args.budget_usd is None:
            raise UsageError("--budget-usd is required for claude (except with --auth local)")
        if args.auth == "api" and args.agent == "claude" and not os.environ.get("ANTHROPIC_API_KEY"):
            raise UsageError("--auth api needs ANTHROPIC_API_KEY (or use --auth login for your Claude login)")
        if args.auth == "api" and args.agent == "codex" and not (os.environ.get("OPENAI_API_KEY") or os.environ.get("CODEX_API_KEY")):
            raise UsageError("--auth api needs OPENAI_API_KEY (or use --auth login)")
        if args.auth == "local" and not args.model:
            raise UsageError("--auth local needs --model, a model the local server has (e.g. qwen3-coder)")
        if args.agent == "codex" and args.auth != "local" and args.max_runs is None:
            raise UsageError("--max-runs is required for codex: it reports no cost, so a budget can't be checked")
        if inside_git(tempfile.gettempdir()):
            raise UsageError(f"{tempfile.gettempdir()} is inside a git repository: set TMPDIR elsewhere")
        if any(a.startswith("lagom:") for a in arms) and not shutil.which(args.lagom_bin):
            raise UsageError(f"Lagom arms need the lagom binary: {args.lagom_bin} not found (--lagom-bin)")
        args.agent_bin = args.agent_bin or (os.path.join(HERE, "fake_agent.py") if args.agent == "fake" else args.agent)
        # Hooks run in the run's folder: a relative path would point nowhere there.
        args.lagom_bin = os.path.abspath(shutil.which(args.lagom_bin) or args.lagom_bin)
    except UsageError as e:
        print(f"ab: {e}", file=sys.stderr)
        return 2

    if args.agent != "fake":
        problems = selftest(tasks)
        if problems:
            print("ab: the selftest fails, so no paid run starts:\n" + "\n".join(problems), file=sys.stderr)
            return 1

    day = datetime.date.today().isoformat()
    args.out = args.out or os.path.join(HERE, "results", day + ("-local" if args.auth == "local" else ""))
    os.makedirs(args.out, exist_ok=True)
    results = os.path.join(args.out, "runs.jsonl")
    spent, rows = 0.0, []
    done = set()
    if args.resume and os.path.exists(results):
        with open(results) as f:
            for r in map(json.loads, f):
                if r.get("kind") == "run" and r.get("status") != "rate_limited":
                    done.add((r["task"], r["arm"], r["repeat"]))
                    spent += r.get("cost_usd") or 0
                    rows.append(r)
    budget_left = lambda: None if args.budget_usd is None else args.budget_usd - spent

    def record(row):
        with open(results, "a") as f:
            f.write(json.dumps(row) + "\n")

    w = warmup(args, budget_left())
    spent += w.get("cost_usd") or 0
    record(w)
    cells = schedule(tasks, arms, args.repeats)
    for n, (repeat, task, arm) in enumerate(cells, 1):
        if (task["name"], arm, repeat) in done:
            continue
        if args.budget_usd is not None and spent >= args.budget_usd:
            print(f"ab: budget of ${args.budget_usd:.2f} reached after {n - 1} of {len(cells)} runs", file=sys.stderr)
            break
        if args.max_runs is not None and n > args.max_runs:
            print(f"ab: --max-runs {args.max_runs} reached", file=sys.stderr)
            break
        row, wait = one_run(args, task, arm, repeat, budget_left()), args.backoff
        for _ in range(args.max_retries):
            if row["status"] != "rate_limited":
                break
            record(row)   # kept, so the results show the pause
            print(f"ab: rate limited; waiting {wait:.0f} s, then retrying {task['name']} {arm}", file=sys.stderr)
            time.sleep(wait)
            row, wait = one_run(args, task, arm, repeat, budget_left()), min(wait * 2, 3600)
        if row["status"] == "rate_limited":
            record(row)
            print(f"ab: still rate limited after {args.max_retries} retries: stopping; rerun with --resume", file=sys.stderr)
            break
        if row.get("cost_usd") is None and args.agent == "claude" and args.auth != "local":
            record(row)
            print(f"ab: run {n} reported no cost ({row['status']}), so spend can't be tracked: stopping", file=sys.stderr)
            break
        spent += row.get("cost_usd") or 0
        rows.append(row)
        record(row)
        print(f"[{n}/{len(cells)}] {task['name']} {arm}: {row['status']}, "
              f"{'passed' if row['passed'] else 'failed'}, spent ${spent:.4f}", file=sys.stderr)
    print(summary(rows, arms, args.auth))
    print(f"\n{len(rows)} runs, ${spent:.4f} spent (warm-up included). Results: {results}")
    print(f"Arms against the baseline, with intervals: python3 -I bench/stats.py {results} [--metric turns]")
    if args.auth == "local":
        problems = smoke_problems(rows)
        print("\n".join(["local smoke check failed:"] + problems) if problems else "local smoke check ok")
        return 1 if problems else 0
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
