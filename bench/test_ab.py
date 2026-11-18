"""The A/B harness against the fake agent: every path, no spend (BEN-06).

    python3 -I -m unittest discover -s bench -p 'test_*.py'
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ab  # noqa: E402

ROOT = os.path.dirname(HERE)
FAKE = os.path.join(HERE, "fake_agent.py")
LAGOM = next((p for p in (os.path.join(ROOT, "target", d, "lagom") for d in ("debug", "release"))
              if os.path.exists(p)), None)


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="lagom-ab-test-")
        self.out = os.path.join(self.tmp, "out")
        self.argv_log = os.path.join(self.tmp, "argv.jsonl")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def ab(self, *args, mode="solve", env=None, tasks=None):
        e = dict(os.environ, FAKE_AGENT_MODE=mode, AB_FAKE_ARGV_LOG=self.argv_log, **(env or {}))
        cmd = [sys.executable, "-I", os.path.join(HERE, "ab.py"), "--tasks", tasks or os.path.join(HERE, "tasks"),
               "--out", self.out, *args]
        return subprocess.run(cmd, capture_output=True, text=True, env=e, timeout=300)

    def rows(self, kind="run"):
        with open(os.path.join(self.out, "runs.jsonl")) as f:
            return [r for r in map(json.loads, f) if r["kind"] == kind]

    def calls(self):
        with open(self.argv_log) as f:
            return [json.loads(line) for line in f]

    def broken_tasks(self):
        """A copy of the sample tasks where the fixture already passes its check."""
        tasks = os.path.join(self.tmp, "tasks")
        shutil.copytree(os.path.join(HERE, "tasks"), tasks)
        shutil.copytree(os.path.join(tasks, "py-add-flag", "solution"), os.path.join(tasks, "py-add-flag", "fixture"),
                        dirs_exist_ok=True)
        return tasks

    def test_selftest_passes_on_the_tasks_and_fails_on_a_broken_one(self):
        self.assertEqual(self.ab("--selftest").returncode, 0)
        r = self.ab("--selftest", tasks=self.broken_tasks())
        self.assertEqual(r.returncode, 1)
        self.assertIn("py-add-flag: the fixture alone passes the hidden check", r.stdout)

    def test_a_dry_run_interleaves_arms_and_records_every_run(self):
        r = self.ab("--agent", "fake", "--arms", "baseline,terse-control", "--repeats", "2")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(self.rows("warmup")), 1)
        rows = self.rows()
        self.assertEqual(len(rows), 2 * 2 * 2)
        self.assertTrue(all(x["status"] == "ok" and x["passed"] for x in rows))
        first_two = [(x["task"], x["arm"]) for x in rows[:2]]
        self.assertEqual(first_two, [("py-add-flag", "baseline"), ("py-add-flag", "terse-control")])
        self.assertEqual(rows[2]["arm"], "terse-control", "the next task starts with the next arm")
        self.assertEqual(rows[0]["usage"], {"input": 100, "output": 200, "cache_read": 3000, "cache_write": 400})

    def test_every_failure_is_recorded_not_dropped(self):
        for mode, status in [("noop", "ok"), ("crash", "bad_output"), ("garbage", "bad_output"),
                             ("error", "agent_error"), ("hang", "timeout")]:
            with self.subTest(mode=mode):
                shutil.rmtree(self.out, ignore_errors=True)
                r = self.ab("--agent", "fake", "--arms", "baseline", "--task", "py-off-by-one",
                            "--timeout", "2", mode=mode)
                self.assertEqual(r.returncode, 0, r.stderr)
                [row] = self.rows()
                self.assertEqual(row["status"], status)
                self.assertFalse(row["passed"])
                self.assertIn("check_tail", row)

    def test_the_budget_stops_new_runs(self):
        r = self.ab("--agent", "fake", "--arms", "baseline,terse-control", "--budget-usd", "0.025")
        self.assertEqual(len(self.rows()), 2, "warm-up 0.01, then two runs reach 0.03")
        self.assertIn("budget of $0.03 reached after 2 of 4 runs", r.stderr)
        self.assertIn("--max-budget-usd", self.calls()[1], "each call is capped at what is left")

    def test_flags_isolate_the_agent_and_never_skip_hooks(self):
        self.ab("--agent", "fake", "--arms", "baseline,terse-control", "--task", "py-add-flag")
        warmup, *runs = self.calls()
        for argv in runs:
            for flag in ab.ISOLATION + ["--output-format", "--permission-mode", "bypassPermissions"]:
                self.assertIn(flag, argv)
            self.assertNotIn("--bare", argv)
            self.assertEqual(argv[argv.index("--disallowedTools") + 1], "Bash", "this task allows no commands")
        terse = [a for a in runs if "--append-system-prompt" in a]
        self.assertEqual(len(terse), 1)
        self.assertEqual(terse[0][terse[0].index("--append-system-prompt") + 1], "Answer concisely.")

    @unittest.skipUnless(LAGOM, "build lagom first (cargo build)")
    def test_a_lagom_arm_installs_the_hooks_and_the_style_is_injected(self):
        r = self.ab("--agent", "fake", "--arms", "lagom:terse", "--task", "py-off-by-one", "--lagom-bin", LAGOM)
        self.assertEqual(r.returncode, 0, r.stderr)
        [row] = self.rows()
        self.assertEqual(row["lagom"]["injections"], 1)

    def test_a_paid_run_refuses_to_start_when_the_selftest_fails(self):
        r = self.ab("--agent", "claude", "--agent-bin", FAKE, "--arms", "baseline", "--budget-usd", "1",
                    tasks=self.broken_tasks())
        self.assertEqual(r.returncode, 1)
        self.assertIn("no paid run starts", r.stderr)
        self.assertFalse(os.path.exists(self.argv_log), "the agent was never called")

    def test_a_paid_run_stops_when_spend_cant_be_tracked(self):
        r = self.ab("--agent", "claude", "--agent-bin", FAKE, "--arms", "baseline", "--budget-usd", "1",
                    mode="garbage")
        self.assertIn("reported no cost", r.stderr)
        self.assertEqual(len(self.rows()), 1)

    def test_usage_errors(self):
        for args, message in [
            (["--agent", "claude"], "--budget-usd is required"),
            (["--agent", "codex"], "--max-runs is required"),
            (["--agent", "fake", "--arms", "lagom"], "unknown arm"),
            (["--agent", "fake", "--arms", "lagom:terse", "--lagom-bin", "/nonexistent/lagom"], "need the lagom binary"),
            (["--agent", "fake", "--task", "nope"], "no such task: nope"),
        ]:
            with self.subTest(args=args):
                r = self.ab(*args)
                self.assertEqual(r.returncode, 2)
                self.assertIn(message, r.stderr)

    def test_runs_never_happen_inside_a_git_repository(self):
        repo = os.path.join(self.tmp, "repo")
        os.makedirs(repo)
        subprocess.run(["git", "init", "-q", repo], check=True)
        r = self.ab("--agent", "fake", "--arms", "baseline", env={"TMPDIR": repo})
        self.assertEqual(r.returncode, 2)
        self.assertIn("inside a git repository", r.stderr)

    def test_codex_commands_and_events(self):
        cmd = ab.command("codex", "codex", "do it", "terse-control", "gpt-x", True, None)
        self.assertEqual(cmd[:2], ["codex", "exec"])
        for flag in ["--json", "--skip-git-repo-check", "--ephemeral", "--ignore-user-config", "workspace-write"]:
            self.assertIn(flag, cmd)
        self.assertIn('developer_instructions="Answer concisely."', cmd)
        self.assertIn("--dangerously-bypass-hook-trust", ab.command("codex", "codex", "x", "lagom:terse", None, True, None))
        events = "\n".join(json.dumps(e) for e in [
            {"type": "thread.started"},
            {"type": "turn.completed", "usage": {"input_tokens": 100, "cached_input_tokens": 60, "output_tokens": 7}},
            {"type": "turn.completed", "usage": {"input_tokens": 50, "cached_input_tokens": 40, "output_tokens": 3}},
        ])
        parsed = ab.parse_codex(events)
        self.assertEqual(parsed["turns"], 2)
        self.assertEqual(parsed["usage"], {"input": 150, "output": 10, "cache_read": 100, "cache_write": 0})
        self.assertIsNone(parsed["cost_usd"])
        self.assertIsNone(ab.parse_codex("not json"))


if __name__ == "__main__":
    unittest.main()
