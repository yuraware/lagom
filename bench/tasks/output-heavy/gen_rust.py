"""The Rust output-heavy tasks (BEN-09).
  warnings write|clean DIR   12 modules under DIR/src with 5 warnings each, or without them
  tests write|check          250+ generated tests in tests/generated.rs; `check` writes them
                             again, over any edits, and runs cargo test. Two fail on the
                             fixture's bugs.
"""
import os
import subprocess
import sys

MODULES = 12


def module(i, clean):
    if clean:
        return f"""/// Doubles `x` and adds {i}.
pub fn compute_{i}(x: i64) -> i64 {{
    let factor = 2;
    x * factor + {i}
}}
"""
    return f"""use std::collections::HashMap;
use std::fmt::Debug;

/// Doubles `x` and adds {i}.
pub fn compute_{i}(x: i64) -> i64 {{
    let tripled = x * 3;
    let mut factor = 2;
    x * factor + {i}
}}

fn helper_{i}() -> i64 {{
    {i}
}}
"""


def write_modules(root, clean):
    os.makedirs(os.path.join(root, "src"), exist_ok=True)
    for i in range(MODULES):
        with open(os.path.join(root, "src", f"m{i}.rs"), "w") as f:
            f.write(module(i, clean))


def generated_tests():
    out = ["use numbers::{clamp_percent, digit_sum, is_leap, parse_hex};", ""]
    for n in range(120):
        v = n * 7919 % 100000
        out += ["#[test]", f"fn digit_sum_{n:03}() {{", f"    assert_eq!(digit_sum({v}), {sum(map(int, str(v)))});", "}", ""]
    for n in range(120):
        y = 1890 + n
        leap = y % 4 == 0 and (y % 100 != 0 or y % 400 == 0)
        out += ["#[test]", f"fn is_leap_{y}() {{", f"    assert_eq!(is_leap({y}), {str(leap).lower()});", "}", ""]
    for v, want in [(-5, 0), (0, 0), (42, 42), (100, 100), (150, 100)]:
        name = f"m{-v}" if v < 0 else str(v)
        out += ["#[test]", f"fn clamp_percent_{name}() {{", f"    assert_eq!(clamp_percent({v}), {want});", "}", ""]
    for s, want in [("0", "Some(0)"), ("ff", "Some(255)"), ("FF", "Some(255)"), ("1a2b", "Some(6699)"), ("xyz", "None")]:
        out += ["#[test]", f"fn parse_hex_{s.lower()}_{len(s)}{'u' if s.isupper() else ''}() {{",
                f'    assert_eq!(parse_hex("{s}"), {want});', "}", ""]
    return "\n".join(out)


if __name__ == "__main__":
    what, mode = sys.argv[1], sys.argv[2]
    if what == "warnings":
        write_modules(sys.argv[3] if len(sys.argv) > 3 else ".", mode == "clean")
    else:
        os.makedirs("tests", exist_ok=True)
        with open("tests/generated.rs", "w") as f:
            f.write(generated_tests())
        if mode == "check":
            sys.exit(subprocess.run(["cargo", "test", "--offline", "--quiet"]).returncode)
