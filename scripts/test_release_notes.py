"""Tests for release_notes.py. Run: python3 -I -m unittest discover -s scripts -p 'test_*.py'"""
import contextlib
import io
import os
import subprocess
import tempfile
import unittest

import release_notes as rn

CHANGELOG = """# Changelog

Intro text.

## [0.1.0] - 2026-11-07

### Added
- First release.
"""


class ReleaseNotesTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        os.mkdir(os.path.join(self.root, "changelog.d"))
        self.write("CHANGELOG.md", CHANGELOG)
        self.write("changelog.d/README.md", "ignored")

    def write(self, rel, text):
        with open(os.path.join(self.root, rel), "w", encoding="utf-8") as fh:
            fh.write(text)

    def read(self, rel):
        with open(os.path.join(self.root, rel), encoding="utf-8") as fh:
            return fh.read()

    def test_three_fragments_make_one_ordered_section(self):
        self.write("changelog.d/HK-02.md", "### Fixed\n- Hooks survive upgrades.\n")
        self.write("changelog.d/FLT-01.md", "### Added\n- Filters.\n  Second line.\n### Fixed\n- A crash.\n")
        self.write("changelog.d/SEC-03.md", "### Security\n- Secrets are redacted.\n")
        rn.release(self.root, "0.2.0", "2026-12-01", dry_run=False)
        text = self.read("CHANGELOG.md")
        expected = (
            "## [0.2.0] - 2026-12-01\n\n### Added\n- Filters.\n  Second line.\n\n"
            "### Fixed\n- A crash.\n- Hooks survive upgrades.\n\n### Security\n- Secrets are redacted.\n\n"
            "## [0.1.0] - 2026-11-07"
        )
        self.assertIn(expected, text)
        self.assertTrue(text.index("Intro text.") < text.index("## [0.2.0]"))
        self.assertEqual(sorted(os.listdir(os.path.join(self.root, "changelog.d"))), ["README.md"])

    def test_dry_run_changes_nothing(self):
        self.write("changelog.d/A.md", "### Added\n- X.\n")
        rn.release(self.root, "0.2.0", "2026-12-01", dry_run=True)
        self.assertEqual(self.read("CHANGELOG.md"), CHANGELOG)
        self.assertTrue(os.path.exists(os.path.join(self.root, "changelog.d/A.md")))

    def test_rejects_bad_fragments_and_duplicate_versions(self):
        for bad in ["### Misc\n- x\n", "- no heading\n", "### Added\nplain text\n", "### Added\n"]:
            with self.assertRaises(rn.FragmentError):
                rn.parse_fragment("bad.md", bad)
        self.write("changelog.d/A.md", "### Added\n- X.\n")
        with self.assertRaises(rn.FragmentError):
            rn.release(self.root, "0.1.0", "2026-12-01", dry_run=True)

    def test_no_fragments_is_an_error(self):
        with self.assertRaises(rn.FragmentError):
            rn.release(self.root, "0.2.0", "2026-12-01", dry_run=True)


class CheckTest(unittest.TestCase):
    """`--check` against a real git repository: base commit on `main`, change on a branch."""

    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "t@example.com")
        self.git("config", "user.name", "t")
        self.write("src/lib.rs", "fn a() {}\n")
        self.write("changelog.d/README.md", "fragments\n")
        self.write("changelog.d/OLD.md", "### Added\n- Old change.\n")
        self.commit("base")
        self.git("checkout", "-q", "-b", "change")
        os.environ.pop("LAGOM_NO_CHANGELOG", None)

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def check(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return rn.check(self.root, "main")

    def test_code_change_needs_a_new_fragment(self):
        self.write("src/lib.rs", "fn b() {}\n")
        self.commit("code")
        self.assertEqual(self.check(), 1)
        self.write("changelog.d/NEW.md", "### Fixed\n- New change.\n")
        self.commit("fragment")
        self.assertEqual(self.check(), 0)

    def test_editing_or_deleting_an_old_fragment_does_not_count(self):
        self.write("src/lib.rs", "fn b() {}\n")
        self.write("changelog.d/OLD.md", "### Added\n- Old change, reworded.\n")
        self.commit("edit")
        self.assertEqual(self.check(), 1)
        os.remove(os.path.join(self.root, "changelog.d/OLD.md"))
        self.commit("delete")
        self.assertEqual(self.check(), 1)

    def test_styles_and_skills_are_user_visible(self):
        self.write("styles/terse.md", "new rules\n")
        self.commit("style")
        self.assertEqual(self.check(), 1)

    def test_docs_only_and_the_label_pass(self):
        self.write("docs/x.md", "doc\n")
        self.commit("docs")
        self.assertEqual(self.check(), 0)
        self.write("src/lib.rs", "fn b() {}\n")
        self.commit("code")
        os.environ["LAGOM_NO_CHANGELOG"] = "1"
        try:
            self.assertEqual(self.check(), 0)
        finally:
            del os.environ["LAGOM_NO_CHANGELOG"]


if __name__ == "__main__":
    unittest.main()
