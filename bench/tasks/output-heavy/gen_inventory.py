"""The inventory tasks' unittest suite (BEN-09): `write N_SKU N_FMT` writes tests/ in the
current folder; `check N_SKU N_FMT` writes the same tests again, over any edits, and runs
them. Three tests fail on the fixture's two bugs, however large the suite."""
import os
import subprocess
import sys

PREFIXES = ["ABC", "XYZ", "QRS", "MNO", "DEF"]


def suite(n_sku, n_fmt):
    out = ["import unittest", "", "from inventory import apply_discount, format_qty, parse_sku, reserve", "", "",
           "class Inventory(unittest.TestCase):"]
    for i in range(n_sku):
        p = PREFIXES[i % len(PREFIXES)]
        out += [f"    def test_sku_{i:04d}(self):", f'        self.assertEqual(parse_sku("{p}-{i:04d}"), ("{p}", {i}))', ""]
    for bad in ["abc-0001", "AB-0001", "ABC_0001", "ABC-01", ""]:
        name = "".join(c if c.isalnum() else "_" for c in bad) or "empty"
        out += [f"    def test_sku_rejects_{name}(self):", "        with self.assertRaises(ValueError):",
                f'            parse_sku("{bad}")', ""]
    for n in range(n_fmt):
        word = "item" if n == 1 else "items"
        out += [f"    def test_qty_{n:03d}(self):", f'        self.assertEqual(format_qty({n}), "{n} {word}")', ""]
    out += ["    def test_discount_round_number(self):", "        self.assertEqual(apply_discount(1000, 10), 900)", "",
            "    def test_discount_half_cent_rounds_up(self):", "        self.assertEqual(apply_discount(999, 50), 500)", "",
            "    def test_discount_ten_percent_of_odd_price(self):", "        self.assertEqual(apply_discount(1005, 10), 905)", "",
            "    def test_reserve_some(self):", "        self.assertEqual(reserve(10, 3), 7)", "",
            "    def test_reserve_more_than_stock(self):", "        with self.assertRaises(ValueError):",
            "            reserve(5, 6)", ""]
    return "\n".join(out) + "\n"


def write(n_sku, n_fmt):
    os.makedirs("tests", exist_ok=True)
    open("tests/__init__.py", "w").close()
    with open("tests/test_inventory.py", "w") as f:
        f.write(suite(n_sku, n_fmt))


if __name__ == "__main__":
    mode, n_sku, n_fmt = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    write(n_sku, n_fmt)
    if mode == "check":
        sys.exit(subprocess.run([sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests", "-t", "."]).returncode)
