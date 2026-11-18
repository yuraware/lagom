#!/usr/bin/env python3
"""Statistics for A/B results (BEN-11): how an arm compares with the baseline, with an
interval, so no number goes out without one. Standard library only.

    python3 -I bench/stats.py bench/results/<date>/runs.jsonl [--metric cost_usd]
        [--baseline baseline] [--resamples 10000] [--seed 1] [--json | --report]

`--report` writes the full Markdown report (BEN-20): every metric per arm, the arms against
the baseline with intervals, a per-task table by category, how runs are counted, and the
limitations.

For each arm and task: the median of the metric over the repeats. Then, per arm, the ratio
of its median to the baseline's on every task both ran; the geometric mean of those ratios
(below 1 means the arm used less); a 95% interval from a paired bootstrap over tasks,
seeded so a rerun gives the same numbers; and a two-sided sign test on how many tasks went
each way. Pass rates come with Wilson 95% intervals.

Metrics: any field of a run, or a dotted path into one (`usage.output`, `turns`,
`wall_ms`, `answer_words`). Runs where the metric is missing (a timeout reports no cost)
are left out of that metric's medians; every run counts for the pass rate.
"""
import argparse
import json
import math
import random
import statistics
import sys

Z95 = 1.959963984540054


def value(row, metric):
    v = row
    for key in metric.split("."):
        v = v.get(key) if isinstance(v, dict) else None
    return v if isinstance(v, (int, float)) and not isinstance(v, bool) else None


def task_medians(rows, metric, arm):
    """{task: median of the metric over the arm's runs of that task}."""
    by_task = {}
    for r in rows:
        v = value(r, metric)
        if r.get("arm") == arm and v is not None:
            by_task.setdefault(r["task"], []).append(v)
    return {t: statistics.median(vs) for t, vs in by_task.items()}


def ratios(arm_medians, base_medians):
    """{task: arm / baseline} for tasks both have, with a positive baseline."""
    return {t: arm_medians[t] / base_medians[t] for t in sorted(arm_medians)
            if t in base_medians and base_medians[t] > 0 and arm_medians[t] > 0}


def geometric_mean(values):
    return math.exp(statistics.fmean(math.log(v) for v in values))


def percentile(sorted_values, q):
    """The q-quantile (0..1) by linear interpolation between order statistics."""
    pos = q * (len(sorted_values) - 1)
    lo, hi = math.floor(pos), math.ceil(pos)
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * (pos - lo)


def bootstrap_interval(values, resamples=10_000, seed=1, level=0.95):
    """A paired bootstrap interval for the geometric mean: tasks are drawn with replacement,
    each bringing its own arm-to-baseline ratio, so task difficulty cancels out."""
    logs = [math.log(v) for v in values]
    rng = random.Random(seed)
    n = len(logs)
    means = sorted(statistics.fmean(rng.choices(logs, k=n)) for _ in range(resamples))
    tail = (1 - level) / 2
    return math.exp(percentile(means, tail)), math.exp(percentile(means, 1 - tail))


def sign_test(values):
    """(tasks below 1, tasks above 1, two-sided exact p) for ratios; ties don't count."""
    below = sum(v < 1 for v in values)
    above = sum(v > 1 for v in values)
    n, k = below + above, min(below, above)
    if n == 0:
        return below, above, 1.0
    p = 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return below, above, min(p, 1.0)


def wilson(passed, total, z=Z95):
    """The Wilson score interval for a pass rate."""
    if total == 0:
        return 0.0, 0.0
    p = passed / total
    denom = 1 + z * z / total
    centre = (p + z * z / (2 * total)) / denom
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / denom
    return max(0.0, centre - half), min(1.0, centre + half)


def compare(rows, metric="cost_usd", baseline="baseline", resamples=10_000, seed=1):
    rows = [r for r in rows if r.get("kind", "run") == "run" and r.get("status") != "rate_limited"]
    arms = sorted({r["arm"] for r in rows}, key=lambda a: (a != baseline, a))
    base = task_medians(rows, metric, baseline)
    result = []
    for arm in arms:
        runs = [r for r in rows if r["arm"] == arm]
        passed = sum(bool(r.get("passed")) for r in runs)
        entry = {"arm": arm, "runs": len(runs), "passed": passed,
                 "pass_rate": passed / len(runs) if runs else 0.0, "pass_interval": wilson(passed, len(runs))}
        if arm != baseline:
            rs = ratios(task_medians(rows, metric, arm), base)
            entry["tasks"] = len(rs)
            if rs:
                values = list(rs.values())
                below, above, p = sign_test(values)
                entry.update(ratio=geometric_mean(values), interval=bootstrap_interval(values, resamples, seed),
                             tasks_below=below, tasks_above=above, sign_p=p, per_task=rs)
        result.append(entry)
    return {"metric": metric, "baseline": baseline, "resamples": resamples, "seed": seed, "arms": result}


def text(report):
    lines = [f"{report['metric']}: arm ÷ {report['baseline']}, geometric mean over tasks of the per-task medians",
             f"{'arm':<20} {'tasks':>5} {'ratio':>7} {'95% interval':>17} {'below/above':>12} {'sign p':>7}  pass rate (95%)"]
    for a in report["arms"]:
        rate = f"{a['passed']}/{a['runs']} {a['pass_rate']:.0%} ({a['pass_interval'][0]:.0%}–{a['pass_interval'][1]:.0%})"
        if "ratio" in a:
            interval = "{:.3f}–{:.3f}".format(*a["interval"])
            split = "{}/{}".format(a["tasks_below"], a["tasks_above"])
            lines.append(f"{a['arm']:<20} {a['tasks']:>5} {a['ratio']:>7.3f} {interval:>17} "
                         f"{split:>12} {a['sign_p']:>7.3f}  {rate}")
        else:
            label = "(baseline)" if a["arm"] == report["baseline"] else "(no shared tasks)"
            lines.append(f"{a['arm']:<20} {a.get('tasks', ''):>5} {label:>25} {'':>20}  {rate}")
    return "\n".join(lines)


REPORT_METRICS = [("cost_usd", "cost"), ("wall_ms", "wall time"), ("api_ms", "API time"), ("turns", "turns"),
                  ("tool_calls", "tool calls"), ("usage.output", "output tokens"), ("usage.input", "uncached input tokens")]
EXERCISED = 2 / 3   # an arm counts only if Lagom acted in this share of its runs (claude-mem's read gate)


def median_or_none(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def fmt(v, spec, none="–"):
    return none if v is None else format(v, spec)


def arm_summary(runs):
    """Every per-arm number of the report, from its runs."""
    n = len(runs)
    passed = sum(bool(r.get("passed")) for r in runs)
    costs = [r["cost_usd"] for r in runs if r.get("cost_usd") is not None]
    tools = {}
    for r in runs:
        for name, count in (r.get("tools") or {}).items():
            tools[name] = tools.get(name, 0) + count
    tool_tokens = sum(r.get("tool_output_bytes") or 0 for r in runs) / 4
    input_tokens = sum(sum((r.get("usage") or {}).get(k, 0) for k in ("input", "cache_read", "cache_write")) for r in runs)
    lagom = [r.get("lagom") for r in runs if r.get("lagom") is not None]
    active = sum(1 for x in lagom if x.get("injections") or x.get("compressed"))
    rewritten = sum(x.get("compressed", 0) for x in lagom)
    risky = [r for r in runs if r.get("risky")]
    return {
        "runs": n, "passed": passed, "pass_interval": wilson(passed, n),
        "timeouts": sum(r.get("status") == "timeout" for r in runs),
        "failed_runs": n - passed,
        "wall_s": median_or_none([r.get("wall_ms") for r in runs]),
        "api_s": median_or_none([r.get("api_ms") for r in runs]),
        "passed_s": median_or_none([r.get("wall_ms") for r in runs if r.get("passed")]),
        "cost_per_pass": sum(costs) / passed if passed and costs else None,
        "turns": median_or_none([r.get("turns") for r in runs]),
        "tool_calls": median_or_none([r.get("tool_calls") for r in runs]),
        "tools": tools,
        "tool_share": tool_tokens / input_tokens if input_tokens else None,
        "exercised": active / len(lagom) if lagom else None,
        "bash_rewritten": rewritten / tools["Bash"] if lagom and tools.get("Bash") else None,
        "risky_with_test": (sum(bool(r.get("added_test")) for r in risky), len(risky)),
        "versions": sorted({r.get("lagom_version") for r in runs if r.get("lagom_version")}),
    }


def report(rows, baseline="baseline", resamples=10_000, seed=1):
    """The Markdown report of an A/B run (BEN-20)."""
    runs = [r for r in rows if r.get("kind", "run") == "run" and r.get("status") != "rate_limited"]
    arms = sorted({r["arm"] for r in runs}, key=lambda a: (a != baseline, a))
    tasks = sorted({r["task"] for r in runs})
    sums = {a: arm_summary([r for r in runs if r["arm"] == a]) for a in arms}
    out = ["# A/B report", ""]
    agents = sorted({str(r.get("agent")) for r in runs})
    auths = sorted({str(r.get("auth")) for r in runs})
    models = sorted({r.get("model") or "the agent's default" for r in runs})
    out.append(f"Agent {', '.join(agents)}, sign-in {', '.join(auths)}, model {', '.join(models)}. "
               f"{len(runs)} runs of {len(tasks)} tasks under {len(arms)} arms, compared with `{baseline}`.")
    for a in arms:
        if sums[a]["versions"]:
            out.append(f"- `{a}` ran {', '.join(sums[a]['versions'])}.")
    out += ["", "## Per arm", "",
            "| arm | passed (95%) | timeouts | median s | median API s | s to a pass | cost per pass | turns | tool calls | tool output share | Bash rewritten | exercised | risky tasks with a test |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for a in arms:
        x = sums[a]
        lo, hi = x["pass_interval"]
        sec = lambda ms: fmt(None if ms is None else ms / 1000, ".1f")
        pct = lambda v: fmt(None if v is None else v * 100, ".0f") + ("%" if v is not None else "")
        rk, rn = x["risky_with_test"]
        out.append(f"| {a} | {x['passed']}/{x['runs']} ({lo:.0%}–{hi:.0%}) | {x['timeouts']} | {sec(x['wall_s'])} | "
                   f"{sec(x['api_s'])} | {sec(x['passed_s'])} | {fmt(x['cost_per_pass'], '.4f')} | {fmt(x['turns'], 'g')} | "
                   f"{fmt(x['tool_calls'], 'g')} | {pct(x['tool_share'])} | {pct(x['bash_rewritten'])} | "
                   f"{pct(x['exercised'])} | {rk}/{rn} |")
    out += ["", "Tool calls by type: " + "; ".join(
        f"{a}: " + (", ".join(f"{k} {v}" for k, v in sorted(sums[a]["tools"].items())) or "none") for a in arms)]
    out += ["", "## Against the baseline", "",
            "Geometric mean over tasks of the arm-to-baseline ratio of per-task medians; below 1 means the arm used less. "
            f"95% intervals from a paired bootstrap over tasks ({resamples} resamples, seed {seed}).", ""]
    for metric, label in REPORT_METRICS:
        cmp = compare(runs, metric, baseline, resamples, seed)
        lines = []
        for a in cmp["arms"]:
            if a["arm"] == baseline or "ratio" not in a:
                continue
            ex = sums[a["arm"]]["exercised"]
            counts = "" if ex is None or ex >= EXERCISED else " (doesn't count: Lagom acted in under two thirds of the runs)"
            lines.append(f"| {a['arm']} | {a['ratio']:.3f} | {a['interval'][0]:.3f}–{a['interval'][1]:.3f} | "
                         f"{a['tasks']} | {a['tasks_below']}/{a['tasks_above']} | {a['sign_p']:.3f} |{counts}")
        if lines:
            out += [f"**{label}** (`{metric}`)", "", "| arm | ratio | 95% interval | tasks | below/above | sign p |",
                    "|---|---:|---|---:|---:|---:|", *lines, ""]
    out += ["## Per task", "", "Passed runs, median seconds and median cost per arm, by kind of task, so losses show.", ""]
    # A task's kind, from any of its runs (older results may lack it).
    kind = {t: next((r["category"] for r in runs if r["task"] == t and r.get("category")), "uncategorized") for t in tasks}
    for category in sorted(set(kind.values())):
        cat_tasks = sorted(t for t in tasks if kind[t] == category)
        out += [f"### {category}", "", "| task | " + " | ".join(arms) + " |", "|---|" + "---|" * len(arms)]
        for t in cat_tasks:
            cells = []
            for a in arms:
                tr = [r for r in runs if r["task"] == t and r["arm"] == a]
                if not tr:
                    cells.append("–")
                    continue
                ms = median_or_none([r.get("wall_ms") for r in tr])
                cost = median_or_none([r.get("cost_usd") for r in tr])
                cells.append(f"{sum(bool(r.get('passed')) for r in tr)}/{len(tr)}, {fmt(None if ms is None else ms / 1000, '.1f')} s, "
                             f"{fmt(cost, '.4f')}")
            out.append(f"| {t} | " + " | ".join(cells) + " |")
        out.append("")
    out += ["## How runs are counted", "",
            "- Every run counts for the pass rate. A run that timed out, crashed or printed output that didn't parse counts as failed.",
            "- A timed-out run's time is the time until it was stopped. It reports no cost, so it is left out of cost medians and "
            "cost per pass, and counted in the timeouts column.",
            "- Rate-limited attempts are retried and not counted; the throwaway warm-up call isn't counted.",
            "- Tool output share: bytes of tool results ÷ 4, over all input tokens (uncached, cache reads and writes).",
            "- Bash rewritten: commands Lagom ran through `lagom run` ÷ Bash calls, for Lagom arms.",
            "- Exercised: the share of an arm's runs in which Lagom injected its style or ran a command. "
            "Under two thirds, the arm's comparisons don't count.",
            "- Risky tasks with a test: runs of tasks marked `risky` before the run in which the agent added or changed a test.",
            "", "## Limitations", ""]
    lim = ["Medians and ratios cover the tasks in this run only; with few tasks, intervals are wide."]
    if "fake" in auths:
        lim.append("A fake agent produced these runs: they test the harness, not any agent or style.")
    if "local" in auths:
        lim.append("A local model produced these runs: they check hooks and parsing, never for publication.")
    if "login" in auths:
        lim.append("Costs under a subscription login are list-price estimates, not what the plan bills.")
    if "codex" in agents:
        lim.append("Codex reports no cost: its cost columns are empty.")
    lim += ["Tool output tokens are estimated from bytes; input tokens come from `modelUsage`, which counts subagents.",
            "Tasks are small and synthetic; real repositories differ."]
    out += [f"- {x}" for x in lim]
    return "\n".join(out) + "\n"


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("runs")
    p.add_argument("--metric", default="cost_usd")
    p.add_argument("--baseline", default="baseline")
    p.add_argument("--resamples", type=int, default=10_000)
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--json", action="store_true")
    p.add_argument("--report", action="store_true", help="write the full Markdown report")
    args = p.parse_args(argv)
    with open(args.runs) as f:
        rows = [json.loads(line) for line in f if line.strip()]
    result = compare(rows, args.metric, args.baseline, args.resamples, args.seed)
    if not any(a["arm"] == args.baseline for a in result["arms"]):
        print(f"stats: no `{args.baseline}` runs to compare with", file=sys.stderr)
        return 2
    if args.report:
        print(report(rows, args.baseline, args.resamples, args.seed), end="")
    else:
        print(json.dumps(result, indent=2) if args.json else text(result))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
