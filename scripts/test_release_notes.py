"""Tests for release_notes.py. Run: python3 -I -m unittest discover -s scripts -p 'test_*.py'"""
import os
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


if __name__ == "__main__":
    unittest.main()
