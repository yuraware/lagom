import json
import os
import subprocess
import tempfile
import unittest

import bench_meta_check as check


def git(root, *args):
    subprocess.run(["git", "-C", root, *args], check=True, capture_output=True)


class BenchMetaCheck(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        git(self.root, "init", "-q", "-b", "main")
        git(self.root, "config", "user.email", "test@example.com")
        git(self.root, "config", "user.name", "test")
        self.write("bench/corpus/git/bad-ref", {"exit_code": 128, "must_keep": ["fatal: bad revision 'x'"]})
        self.write("tests/fixtures/compress/gap", {"exit_code": 0, "must_keep": ["a.rs"], "known_gap": "FLT-07"})
        git(self.root, "add", "-A")
        git(self.root, "commit", "-q", "-m", "base")
        git(self.root, "checkout", "-q", "-b", "pr")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, case, meta, name="meta.json"):
        os.makedirs(f"{self.root}/{case}", exist_ok=True)
        with open(f"{self.root}/{case}/{name}", "w") as f:
            f.write(json.dumps(meta) + "\n" if isinstance(meta, dict) else meta)

    def run_check(self, label=False):
        env = {"LAGOM_BENCH_META_OK": "1"} if label else {}
        return check.main(["--base", "main", "--root", self.root], env)

    def test_rewording_a_must_keep_fails_without_the_label(self):
        self.write("bench/corpus/git/bad-ref", {"exit_code": 128, "must_keep": ["fatal:"]})
        self.assertEqual(self.run_check(), 1)
        self.assertEqual(self.run_check(label=True), 0)

    def test_changing_a_known_gap_or_removing_a_case_fails(self):
        self.write("tests/fixtures/compress/gap", {"exit_code": 0, "must_keep": ["a.rs"], "known_gap": "FLT-99"})
        self.assertEqual(self.run_check(), 1)
        git(self.root, "checkout", "-q", "--", ".")
        os.remove(f"{self.root}/bench/corpus/git/bad-ref/meta.json")
        self.assertEqual(self.run_check(), 1)

    def test_new_cases_and_other_edits_pass(self):
        self.write("bench/corpus/go/new-case", {"exit_code": 1, "must_keep": ["FAIL"]})
        self.write("bench/corpus/git/bad-ref", "fatal: bad revision 'x'\n", name="input.txt")
        self.assertEqual(self.run_check(), 0)

    def test_a_committed_change_is_found_too(self):
        self.write("bench/corpus/git/bad-ref", {"exit_code": 128, "must_keep": ["bad revision"]})
        git(self.root, "commit", "-q", "-am", "reword")
        self.assertEqual(self.run_check(), 1)


if __name__ == "__main__":
    unittest.main()
