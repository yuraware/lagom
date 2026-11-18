"""bench/report.py: the page is rebuilt byte for byte from its results (BEN-12).

    BENCH_BLESS=1 python3 -I -m unittest discover -s bench -p test_report.py   # after an intended change
"""
import os
import shutil
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import report  # noqa: E402

SAMPLE = "bench/testdata/sample-runs.jsonl"   # synthetic runs, not a measurement
EXPECTED = os.path.join(HERE, "testdata", "report")


class Report(unittest.TestCase):
    def test_the_sample_page_is_reproduced_byte_for_byte(self):
        files = report.build(SAMPLE, resamples=2000)
        if os.environ.get("BENCH_BLESS") == "1":
            report.write(files, EXPECTED)
        for name, content in files.items():
            with self.subTest(file=name), open(os.path.join(EXPECTED, name)) as f:
                self.assertEqual(f.read(), content, f"rerun with BENCH_BLESS=1 if {name} changed on purpose")

    def test_every_arm_and_three_separate_numbers(self):
        page = report.build(SAMPLE, resamples=2000)["benchmarks.md"]
        for arm in ("baseline", "terse-control", "lagom:terse"):
            self.assertIn(f"| {arm} |", page)
        self.assertIn("| arm | passed (95%) | cost per passed task | turns (median) |", page)
        self.assertNotIn("score", page.split("## Per arm")[0].replace("never one score", ""))

    def test_check_passes_on_the_page_and_fails_on_an_edit(self):
        out = tempfile.mkdtemp(prefix="lagom-report-check-")
        try:
            self.assertEqual(report.check(out), 0, "no page yet: nothing to check")
            report.write(report.build(SAMPLE, resamples=2000), out)
            self.assertEqual(report.check(out), 0)
            with open(os.path.join(out, "benchmarks", "cost-ratio.svg"), "a") as f:
                f.write("<!-- edited -->\n")
            self.assertEqual(report.check(out), 1)
        finally:
            shutil.rmtree(out)

    def test_the_readme_block_has_the_same_numbers(self):
        page = report.build(SAMPLE, resamples=2000)["benchmarks.md"]
        block = report.readme_block(page)
        self.assertTrue(block.startswith(report.START) and block.endswith(report.END))
        self.assertIn(page.split("\n\n")[2], block)

    def test_the_charts_are_svg_with_whiskers(self):
        files = report.build(SAMPLE, resamples=2000)
        for name in ("benchmarks/pass-rate.svg", "benchmarks/cost-ratio.svg", "benchmarks/turns-ratio.svg"):
            svg = files[name]
            self.assertTrue(svg.startswith("<svg ") and svg.rstrip().endswith("</svg>"), name)
            self.assertGreaterEqual(svg.count("<line"), 3 * 2, name)


if __name__ == "__main__":
    unittest.main()
