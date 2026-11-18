#!/usr/bin/env python3
"""Statistics for A/B results (BEN-11): how an arm compares with the baseline, with an
interval, so no number goes out without one. Standard library only.

    python3 -I bench/stats.py bench/results/<date>/runs.jsonl [--metric cost_usd]
        [--baseline baseline] [--resamples 10000] [--seed 1] [--json]

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


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("runs")
    p.add_argument("--metric", default="cost_usd")
    p.add_argument("--baseline", default="baseline")
    p.add_argument("--resamples", type=int, default=10_000)
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--json", action="store_true")
    args = p.parse_args(argv)
    with open(args.runs) as f:
        rows = [json.loads(line) for line in f if line.strip()]
    report = compare(rows, args.metric, args.baseline, args.resamples, args.seed)
    if not any(a["arm"] == args.baseline for a in report["arms"]):
        print(f"stats: no `{args.baseline}` runs to compare with", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2) if args.json else text(report))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
