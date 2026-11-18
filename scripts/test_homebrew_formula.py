"""Tests for homebrew_formula.py. Run: python3 -I -m unittest discover -s scripts -p 'test_*.py'"""
import os
import shutil
import subprocess
import tempfile
import unittest

import homebrew_formula as hf

TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
]


def sums(version="0.2.0", skip=None):
    lines = []
    for i, t in enumerate(TARGETS):
        if t != skip:
            lines.append(f"{str(i) * 64}  lagom-{version}-{t}.tar.gz")
    return "\n".join(lines) + "\n"


class HomebrewFormulaTest(unittest.TestCase):
    def test_each_platform_gets_its_archive_and_hash(self):
        f = hf.render("0.2.0", hf.parse_sums(sums()))
        base = "https://github.com/yuraware/lagom/releases/download/v0.2.0"
        for i, t in enumerate(TARGETS):
            self.assertIn(f'url "{base}/lagom-0.2.0-{t}.tar.gz"', f)
            self.assertIn(f'sha256 "{str(i) * 64}"', f)
        # The Arm Mac archive sits in the macOS block's on_arm branch, before on_intel.
        mac = f[f.index("on_macos do") : f.index("on_linux do")]
        self.assertLess(mac.index("on_arm do"), mac.index("aarch64-apple-darwin"))
        self.assertLess(mac.index("aarch64-apple-darwin"), mac.index("on_intel do"))
        self.assertIn('version "0.2.0"', f)
        self.assertNotIn('"rust" => :build\n  def', f)  # rust only for --HEAD builds

    def test_missing_archive_is_refused(self):
        with self.assertRaises(hf.FormulaError):
            hf.render("0.2.0", hf.parse_sums(sums(skip="x86_64-apple-darwin")))

    def test_pre_release_and_bad_versions_are_refused(self):
        for v in ["0.2.0-rc.1", "v0.2.0", "0.2", ""]:
            with self.assertRaises(hf.FormulaError, msg=v):
                hf.render(v, hf.parse_sums(sums(v)))

    def test_malformed_hash_is_refused(self):
        with self.assertRaises(hf.FormulaError):
            hf.parse_sums("xyz  lagom-0.2.0-x86_64-apple-darwin.tar.gz\n")

    def test_binary_mode_marker_is_accepted(self):
        parsed = hf.parse_sums(f"{'a' * 64} *lagom-0.2.0-aarch64-apple-darwin.tar.gz\n")
        self.assertEqual(parsed, {"lagom-0.2.0-aarch64-apple-darwin.tar.gz": "a" * 64})

    @unittest.skipUnless(shutil.which("ruby"), "ruby not installed")
    def test_formula_is_valid_ruby(self):
        with tempfile.NamedTemporaryFile("w", suffix=".rb", delete=False) as fh:
            fh.write(hf.render("0.2.0", hf.parse_sums(sums())))
        try:
            out = subprocess.run(["ruby", "-c", fh.name], capture_output=True, text=True)
            self.assertEqual(out.returncode, 0, out.stderr)
        finally:
            os.unlink(fh.name)


if __name__ == "__main__":
    unittest.main()
