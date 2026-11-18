"""bench/stats.py against values known in closed form (BEN-11)."""
import math
import os
import statistics
import subprocess
import sys
import tempfile
import json
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import stats  # noqa: E402


class Bootstrap(unittest.TestCase):
    def test_identical_ratios_have_a_zero_width_interval(self):
        lo, hi = stats.bootstrap_interval([0.8] * 12, resamples=2000)
        self.assertAlmostEqual(lo, 0.8)
        self.assertAlmostEqual(hi, 0.8)

    def test_two_tasks_span_exactly_their_two_ratios(self):
        # Resampling two tasks gives {a, a}, {a, b} or {b, b}: a quarter of the means are
        # each extreme, so the 2.5% and 97.5% points are the two ratios themselves.
        lo, hi = stats.bootstrap_interval([0.5, 2.0], resamples=4000, seed=7)
        self.assertAlmostEqual(lo, 0.5)
        self.assertAlmostEqual(hi, 2.0)

    def test_the_standard_error_matches_sigma_over_root_n(self):
        # For the mean of n values, the bootstrap's standard error is the population
        # standard deviation over sqrt(n); a normal 95% interval is about ±1.96 of it.
        logs = [math.sin(i) for i in range(200)]
        values = [math.exp(x) for x in logs]
        se = statistics.pstdev(logs) / math.sqrt(len(logs))
        lo, hi = stats.bootstrap_interval(values, resamples=20_000, seed=3)
        centre = statistics.fmean(logs)
        self.assertAlmostEqual(math.log(hi) - centre, stats.Z95 * se, delta=0.1 * stats.Z95 * se)
        self.assertAlmostEqual(centre - math.log(lo), stats.Z95 * se, delta=0.1 * stats.Z95 * se)

    def test_the_same_seed_gives_the_same_interval(self):
        # Enough distinct tasks that two seeds can't land on the same percentiles.
        values = [0.5 + i / 37 for i in range(40)]
        self.assertEqual(stats.bootstrap_interval(values, 500, seed=5), stats.bootstrap_interval(values, 500, seed=5))
        self.assertNotEqual(stats.bootstrap_interval(values, 500, seed=5), stats.bootstrap_interval(values, 500, seed=6))


class Tests(unittest.TestCase):
    def test_geometric_mean(self):
        self.assertAlmostEqual(stats.geometric_mean([0.5, 2.0]), 1.0)
        self.assertAlmostEqual(stats.geometric_mean([1, 2, 4]), 2.0)

    def test_sign_test_exact_values(self):
        self.assertEqual(stats.sign_test([0.5] * 10), (10, 0, 2 / 1024))
        # 8 of 10 below: 2 × (1 + 10 + 45) / 1024.
        below, above, p = stats.sign_test([0.9] * 8 + [1.2] * 2 + [1.0])
        self.assertEqual((below, above), (8, 2))
        self.assertAlmostEqual(p, 0.109375)
        self.assertEqual(stats.sign_test([1.0, 1.0])[2], 1.0)

    def test_wilson_known_values(self):
        lo, hi = stats.wilson(8, 10)
        self.assertAlmostEqual(lo, 0.4902, places=4)
        self.assertAlmostEqual(hi, 0.9433, places=4)
        self.assertEqual(stats.wilson(0, 0), (0.0, 0.0))
        lo, hi = stats.wilson(10, 10)
        self.assertAlmostEqual(hi, 1.0)
        self.assertAlmostEqual(lo, 0.7225, places=4)


class Report(unittest.TestCase):
    def rows(self):
        out = []
        for task, base, arm in [("t1", 1.0, 0.5), ("t2", 2.0, 1.0), ("t3", 4.0, 2.0)]:
            for rep, jitter in enumerate([0.9, 1.0, 1.1]):
                out.append({"kind": "run", "task": task, "arm": "baseline", "repeat": rep, "passed": True,
                            "cost_usd": base * jitter, "usage": {"output": 100}})
                out.append({"kind": "run", "task": task, "arm": "lagom:terse", "repeat": rep, "passed": rep != 0,
                            "cost_usd": arm * jitter, "usage": {"output": 50}})
        out.append({"kind": "warmup", "cost_usd": 9.9})
        out.append({"kind": "run", "task": "t1", "arm": "lagom:terse", "repeat": 3, "passed": False,
                    "status": "timeout", "cost_usd": None})
        return out

    def test_an_arm_that_halves_every_task(self):
        report = stats.compare(self.rows(), resamples=2000)
        base, arm = report["arms"]
        self.assertEqual(base["arm"], "baseline")
        self.assertAlmostEqual(arm["ratio"], 0.5)
        self.assertAlmostEqual(arm["interval"][0], 0.5)
        self.assertAlmostEqual(arm["interval"][1], 0.5)
        self.assertEqual((arm["tasks_below"], arm["tasks_above"]), (3, 0))
        self.assertEqual(arm["runs"], 10, "the timeout counts as a run, the warm-up doesn't")
        self.assertEqual(arm["passed"], 6)

    def test_dotted_metrics_and_the_command_line(self):
        report = stats.compare(self.rows(), metric="usage.output", resamples=500)
        self.assertAlmostEqual(report["arms"][1]["ratio"], 0.5)
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as f:
            f.write("\n".join(json.dumps(r) for r in self.rows()))
        try:
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "stats.py"), f.name, "--resamples", "500"],
                               capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertIn("lagom:terse", r.stdout)
            self.assertIn("0.500", r.stdout)
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "stats.py"), f.name, "--baseline", "nope"],
                               capture_output=True, text=True)
            self.assertEqual(r.returncode, 2)
        finally:
            os.remove(f.name)


if __name__ == "__main__":
    unittest.main()
